//! The Look section of the persona form: the character the persona
//! appears as in performed clips. Shows the look and a drawing waiting for
//! review beside it, draws one from a description (with the budget
//! question), imports a photo of the user once they confirm it shows them,
//! and marks the look realistic or removes it. Where a take model takes
//! looks from its provider's asset library (BytePlus), it also keeps the
//! asset id that stands for the look. Rules, storage and the drawing job
//! live in `bardo_app`; this file maps them to the screen.

use std::sync::Arc;
use std::time::Duration;

use bardo_app::bardo_domain::{
    ImageFormat, JobFailureKind, JobId, JobState, LookAssetKind, LookImageError, LookSource,
    PersonaId, PersonaLook, Provider,
};
use bardo_app::{Bardo, BudgetConsent, LookError, LookView, SpendEstimate, Text};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Selectable as _, Sizable as _, StyledExt as _, h_flex,
    v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Entity, Image, ObjectFit, PathPromptOptions, SharedString,
    Subscription, Task, Window, div, img, px,
};

use crate::guide;
use crate::kit::{self, Tone};
use crate::shell::tr;
use crate::spend::{budget_question, estimate_note};

/// How often a drawing in progress is checked.
const POLL_EVERY: Duration = Duration::from_millis(250);

/// The width of a look's picture; its frame is 2:3.
const PICTURE_WIDTH: f32 = 120.;

/// What the section says after the last action.
#[derive(Clone, Copy)]
enum Notice {
    Kept,
    Imported,
    Removed,
    AssetSaved,
    AssetRemoved,
    /// The action failed, and, for a refused photo, why.
    Error(Text, Option<LookImageError>),
}

/// A look ready to show: the stored look and its decoded picture.
struct Shown {
    look: PersonaLook,
    picture: Arc<Image>,
}

impl Shown {
    fn of(look: PersonaLook) -> Self {
        let format = match look.image.format() {
            ImageFormat::Jpeg => gpui_kit::ImageFormat::Jpeg,
            ImageFormat::Png | ImageFormat::Webp => gpui_kit::ImageFormat::Png,
        };
        let picture = Arc::new(Image::from_bytes(format, look.image.bytes().to_vec()));
        Self { look, picture }
    }
}

pub struct LookSection {
    bardo: Entity<Bardo>,
    /// The persona being edited; `None` while creating one.
    persona: Option<PersonaId>,
    /// The section's facts, without the pictures (kept in `look` and
    /// `pending`, decoded once).
    view: Option<LookView>,
    /// The section could not be read.
    load_failed: bool,
    /// The drawing job last seen, by id and state: the section reloads
    /// when it moves, not on every other job's progress.
    seen_job: Option<(JobId, JobState)>,
    /// A finished job whose outcome the user moved past (kept, discarded,
    /// imported or removed a look since): its line no longer shows.
    dismissed: Option<JobId>,
    look: Option<Shown>,
    pending: Option<Shown>,
    description: Entity<TextareaState>,
    description_error: Option<Text>,
    /// The asset id typed for the look, and the kind picked for it.
    asset_id: Entity<InputState>,
    asset_kind: Option<LookAssetKind>,
    /// A drawing past the budget, waiting for the user's answer.
    ask: Option<SpendEstimate>,
    /// The import card is open, with whether the user ticked that the
    /// photo shows them.
    importing: Option<bool>,
    /// Removing the look waits for the user's confirmation.
    removing: bool,
    notice: Option<Notice>,
    /// The photo dialog that is open.
    dialog: Option<Task<()>>,
    revision: u64,
    _poll: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl LookSection {
    pub fn new(bardo: Entity<Bardo>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let description = cx.new(|cx| TextareaState::new(window, cx).auto_grow(2, 6));
        let asset_id = cx.new(|cx| InputState::new(window, cx));
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL_EVERY).await;
                if this.update(cx, |this, cx| this.poll(cx)).is_err() {
                    break;
                }
            }
        });
        let subscriptions = vec![
            cx.subscribe(&description, |this, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    this.description_error = None;
                    this.ask = None;
                    cx.notify();
                }
            }),
            cx.observe_in(&bardo, window, |this, _, window, cx| {
                this.relabel(window, cx)
            }),
            cx.subscribe(&asset_id, |this, _, event, cx| {
                if matches!(event, InputEvent::Change)
                    && matches!(this.notice, Some(Notice::Error(..)))
                {
                    this.notice = None;
                    cx.notify();
                }
            }),
        ];
        let revision = bardo.read(cx).jobs_revision();
        let mut section = Self {
            bardo,
            persona: None,
            view: None,
            load_failed: false,
            seen_job: None,
            dismissed: None,
            look: None,
            pending: None,
            description,
            description_error: None,
            asset_id,
            asset_kind: None,
            ask: None,
            importing: None,
            removing: false,
            notice: None,
            dialog: None,
            revision,
            _poll: poll,
            _subscriptions: subscriptions,
        };
        section.relabel(window, cx);
        section
    }

    /// Shows the look of `persona`, or the note to save it first.
    pub fn show(
        &mut self,
        persona: Option<PersonaId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let changed = self.persona != persona;
        self.persona = persona;
        if changed {
            self.description_error = None;
            self.ask = None;
            self.importing = None;
            self.removing = false;
            self.notice = None;
            self.dialog = None;
            self.asset_kind = None;
            self.asset_id
                .update(cx, |input, cx| input.set_value("", window, cx));
        }
        self.load(cx);
        if changed {
            // Start from what the look was drawn from, to vary it.
            let drawn = [&self.pending, &self.look]
                .into_iter()
                .flatten()
                .find_map(|shown| shown.look.drawing.as_ref())
                .map(|drawing| drawing.description.clone())
                .unwrap_or_default();
            self.description
                .update(cx, |input, cx| input.set_value(drawn, window, cx));
        }
        cx.notify();
    }

    fn relabel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let placeholder = tr(self.bardo.read(cx), Text::LookDescriptionPlaceholder);
        self.description.update(cx, |input, cx| {
            input.set_placeholder(placeholder, window, cx)
        });
        let placeholder = tr(self.bardo.read(cx), Text::LookAssetIdPlaceholder);
        self.asset_id.update(cx, |input, cx| {
            input.set_placeholder(placeholder, window, cx)
        });
        cx.notify();
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let Some(persona) = self.persona else {
            self.view = None;
            self.look = None;
            self.pending = None;
            return;
        };
        match self.bardo.read(cx).persona_look(persona) {
            Ok(mut view) => {
                // Decode a picture only when it changed.
                let same = |shown: &Option<Shown>, look: &Option<PersonaLook>| match (shown, look) {
                    (Some(shown), Some(look)) => shown.look == *look,
                    (None, None) => true,
                    _ => false,
                };
                if !same(&self.look, &view.look) {
                    self.look = view.look.clone().map(Shown::of);
                }
                if !same(&self.pending, &view.pending) {
                    self.pending = view.pending.clone().map(Shown::of);
                }
                // The pictures live in `look` and `pending`; the view is
                // cloned on every render.
                view.look = None;
                view.pending = None;
                self.seen_job = view.job.as_ref().map(|job| (job.id(), job.state()));
                self.view = Some(view);
                self.load_failed = false;
            }
            Err(_) => {
                self.view = None;
                self.look = None;
                self.pending = None;
                self.load_failed = true;
            }
        }
    }

    /// Reloads when this persona's drawing job moved.
    fn poll(&mut self, cx: &mut Context<Self>) {
        let Some(persona) = self.persona else {
            return;
        };
        let revision = self.bardo.read(cx).jobs_revision();
        if revision == self.revision {
            return;
        }
        self.revision = revision;
        let job = self
            .bardo
            .read(cx)
            .persona_look_job(persona)
            .map(|job| (job.id(), job.state()));
        if job != self.seen_job {
            self.load(cx);
            cx.notify();
        }
    }

    /// The latest job's outcome no longer applies.
    fn dismiss_job(&mut self) {
        self.dismissed = self
            .view
            .as_ref()
            .and_then(|view| view.job.as_ref())
            .map(|job| job.id());
    }

    fn fail(&mut self, error: &LookError, cx: &mut Context<Self>) {
        if let LookError::Description(error) = error {
            self.description_error = Some(Text::LookDescriptionError(*error));
        } else {
            self.notice = Some(Notice::Error(error.message(), error.image_error()));
        }
        cx.notify();
    }

    /// Draws a look from the description; past the budget, asks first
    /// unless `consent` says the user already agreed.
    fn draw(&mut self, consent: BudgetConsent, cx: &mut Context<Self>) {
        let Some(persona) = self.persona else {
            return;
        };
        let description = self.description.read(cx).value().to_string();
        self.notice = None;
        self.ask = None;
        match self
            .bardo
            .read(cx)
            .draw_look(persona, &description, consent)
        {
            Ok(_) => {
                self.description_error = None;
                self.load(cx);
            }
            Err(LookError::OverBudget(estimate)) => self.ask = Some(estimate),
            Err(error) => self.fail(&error, cx),
        }
        cx.notify();
    }

    fn keep(&mut self, cx: &mut Context<Self>) {
        let Some(persona) = self.persona else {
            return;
        };
        match self.bardo.read(cx).keep_drawn_look(persona) {
            Ok(_) => {
                self.dismiss_job();
                self.notice = Some(Notice::Kept);
            }
            Err(error) => self.fail(&error, cx),
        }
        self.load(cx);
        cx.notify();
    }

    fn discard(&mut self, cx: &mut Context<Self>) {
        let Some(persona) = self.persona else {
            return;
        };
        self.notice = None;
        match self.bardo.read(cx).discard_drawn_look(persona) {
            Ok(()) => self.dismiss_job(),
            Err(error) => self.fail(&error, cx),
        }
        self.load(cx);
        cx.notify();
    }

    fn remove(&mut self, cx: &mut Context<Self>) {
        let Some(persona) = self.persona else {
            return;
        };
        self.removing = false;
        match self.bardo.read(cx).remove_look(persona) {
            Ok(()) => {
                self.dismiss_job();
                self.notice = Some(Notice::Removed);
            }
            Err(error) => self.fail(&error, cx),
        }
        self.load(cx);
        cx.notify();
    }

    fn set_realistic(&mut self, realistic: bool, cx: &mut Context<Self>) {
        let Some(persona) = self.persona else {
            return;
        };
        self.notice = None;
        if let Err(error) = self.bardo.read(cx).set_look_realistic(persona, realistic) {
            self.fail(&error, cx);
        }
        self.load(cx);
        cx.notify();
    }

    /// Asks for the photo, then makes it the look.
    fn choose_photo(&mut self, cx: &mut Context<Self>) {
        let (Some(persona), Some(shows_me)) = (self.persona, self.importing) else {
            return;
        };
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(tr(self.bardo.read(cx), Text::LookImportDialog)),
        });
        self.notice = None;
        self.dialog = Some(cx.spawn(async move |this, cx| {
            let chosen = chosen.await;
            let _ = this.update(cx, |this, cx| {
                this.dialog = None;
                match chosen {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.first() {
                            match this.bardo.read(cx).import_look(persona, path, shows_me) {
                                Ok(_) => {
                                    this.importing = None;
                                    this.dismiss_job();
                                    this.notice = Some(Notice::Imported);
                                }
                                Err(error) => this.fail(&error, cx),
                            }
                            this.load(cx);
                        }
                    }
                    Ok(Ok(None)) | Err(_) => {}
                    Ok(Err(_)) => {
                        this.notice = Some(Notice::Error(Text::FileDialogFailed, None));
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// A look's picture in its 2:3 frame.
    fn picture(shown: &Shown, cx: &App) -> AnyElement {
        div()
            .flex_none()
            .w(px(PICTURE_WIDTH))
            .h(px(PICTURE_WIDTH * 1.5))
            .rounded_md()
            .overflow_hidden()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().muted)
            .child(
                img(shown.picture.clone())
                    .size_full()
                    .object_fit(ObjectFit::Contain),
            )
            .into_any_element()
    }

    /// The description a look was drawn from, small under its tag.
    fn drawn_from(bardo: &Bardo, look: &PersonaLook, cx: &App) -> Option<AnyElement> {
        let drawing = look
            .drawing
            .as_ref()
            .filter(|d| !d.description.is_empty())?;
        Some(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(SharedString::from(bardo.text_with(
                    Text::LookFromDescription,
                    &[("description", &drawing.description)],
                )))
                .into_any_element(),
        )
    }

    fn render_look(&self, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let Some(shown) = &self.look else {
            return div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(tr(bardo, Text::LookEmpty))
                .into_any_element();
        };
        let look = &shown.look;
        let tag = match look.source {
            LookSource::Drawn => kit::status(Tone::Accent, tr(bardo, Text::LookDrawnTag), cx),
            LookSource::Imported => kit::status(Tone::Info, tr(bardo, Text::LookImportedTag), cx),
        };
        let realistic = h_flex()
            .gap_1()
            .items_center()
            .child(
                Checkbox::new("persona-look-realistic")
                    .min_w_0()
                    .checked(look.realistic)
                    // A photo of you is a real person.
                    .disabled(look.source == LookSource::Imported)
                    .label(tr(bardo, Text::LookRealistic))
                    .on_click(
                        cx.listener(|this, checked: &bool, _, cx| this.set_realistic(*checked, cx)),
                    ),
            )
            .child(guide::info(
                bardo,
                "persona-look-realistic-info",
                tr(bardo, Text::LookRealisticHint),
                guide::refs::PERSONAS_LOOK,
            ));
        let remove: AnyElement = if self.removing {
            v_flex()
                .gap_2()
                .child(div().text_sm().child(tr(
                    bardo,
                    match look.source {
                        LookSource::Drawn => Text::LookRemoveConfirm,
                        LookSource::Imported => Text::LookRemovePhotoConfirm,
                    },
                )))
                .child(
                    h_flex()
                        .flex_wrap()
                        .gap_2()
                        .child(
                            Button::new("confirm-remove-look")
                                .small()
                                .danger()
                                .label(tr(bardo, Text::LookRemoveYes))
                                .on_click(
                                    cx.listener(|this, _: &ClickEvent, _, cx| this.remove(cx)),
                                ),
                        )
                        .child(
                            Button::new("cancel-remove-look")
                                .small()
                                .ghost()
                                .label(tr(bardo, Text::LookCancel))
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.removing = false;
                                    cx.notify();
                                })),
                        ),
                )
                .into_any_element()
        } else {
            h_flex()
                .child(
                    Button::new("remove-look")
                        .small()
                        .ghost()
                        .label(tr(bardo, Text::LookRemove))
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.removing = true;
                            this.notice = None;
                            cx.notify();
                        })),
                )
                .into_any_element()
        };
        h_flex()
            .flex_wrap()
            .items_start()
            .gap_3()
            .child(Self::picture(shown, cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(200.))
                    .gap_2()
                    .child(h_flex().child(tag))
                    .children(Self::drawn_from(bardo, look, cx))
                    .child(realistic)
                    .child(remove),
            )
            .into_any_element()
    }

    /// The drawing waiting for review, with Keep and Discard.
    fn render_pending(&self, shown: &Shown, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        kit::well(cx)
            .p_3()
            .child(
                h_flex()
                    .flex_wrap()
                    .items_start()
                    .gap_3()
                    .child(Self::picture(shown, cx))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(200.))
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .font_medium()
                                    .child(tr(bardo, Text::LookPendingTitle)),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(tr(bardo, Text::LookPendingHint)),
                            )
                            .children(Self::drawn_from(bardo, &shown.look, cx))
                            .child(
                                h_flex()
                                    .flex_wrap()
                                    .gap_2()
                                    .child(
                                        Button::new("keep-look")
                                            .small()
                                            .primary()
                                            .label(tr(bardo, Text::LookKeep))
                                            .on_click(cx.listener(
                                                |this, _: &ClickEvent, _, cx| this.keep(cx),
                                            )),
                                    )
                                    .child(
                                        Button::new("discard-look")
                                            .small()
                                            .ghost()
                                            .label(tr(bardo, Text::LookDiscard))
                                            .on_click(cx.listener(
                                                |this, _: &ClickEvent, _, cx| this.discard(cx),
                                            )),
                                    ),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// What the latest drawing is doing, or why it did not finish.
    fn render_job(&self, view: &LookView, cx: &mut Context<Self>) -> Option<AnyElement> {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let job = view.job.as_ref()?;
        if self.dismissed == Some(job.id()) {
            return None;
        }
        let line = match job.state() {
            JobState::Queued | JobState::Running => {
                return Some(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(Spinner::new().small())
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme.muted_foreground)
                                .child(tr(bardo, Text::LookDrawing)),
                        )
                        .into_any_element(),
                );
            }
            JobState::Done => return None,
            JobState::Cancelled => kit::notice(Tone::Neutral, tr(bardo, Text::LookStopped), cx),
            JobState::Failed => match job.failure() {
                Some(failure) if failure.kind == JobFailureKind::Declined => kit::notice(
                    Tone::Warning,
                    bardo.text_with(Text::LookDeclined, &[("reason", &failure.detail)]),
                    cx,
                ),
                _ => kit::notice(Tone::Danger, tr(bardo, Text::LookFailed), cx),
            },
        };
        Some(line.into_any_element())
    }

    fn render_draw(&self, view: &LookView, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let drawing = view.is_drawing();
        let again = self.look.is_some() || self.pending.is_some();
        let ask = self.ask.as_ref().map(|estimate| {
            budget_question(
                "persona-look-budget",
                bardo,
                estimate,
                cx,
                cx.listener(|this, _: &ClickEvent, _, cx| this.draw(BudgetConsent::Confirmed, cx)),
                cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.ask = None;
                    cx.notify();
                }),
            )
        });
        let error = self.description_error.map(|error| {
            div()
                .text_xs()
                .text_color(theme.danger)
                .child(tr(bardo, error))
        });
        v_flex()
            .gap_2()
            .child(kit::field(
                tr(bardo, Text::LookDescription),
                None,
                Textarea::new(&self.description).into_any_element(),
                error.map(IntoElement::into_any_element),
            ))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(SharedString::from(bardo.text_with(
                        Text::LookDrawHint,
                        &[("n", &view.template.number.to_string())],
                    ))),
            )
            .children(estimate_note(bardo, &view.estimate, Text::EstimateCost, cx))
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        Button::new("draw-look")
                            .small()
                            .outline()
                            .label(tr(
                                bardo,
                                if again {
                                    Text::LookDrawAgain
                                } else {
                                    Text::LookDraw
                                },
                            ))
                            .loading(drawing)
                            .disabled(drawing || self.ask.is_some())
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.draw(BudgetConsent::Ask, cx)
                            })),
                    )
                    .when(self.importing.is_none(), |row| {
                        row.child(
                            Button::new("import-look")
                                .small()
                                .ghost()
                                .label(tr(bardo, Text::LookImport))
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.importing = Some(false);
                                    this.notice = None;
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .children(ask)
            .into_any_element()
    }

    /// The photo import: what the photo must be, and the user's word that
    /// it shows them.
    fn render_import(&self, shows_me: bool, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        kit::card(cx)
            .p_3()
            .gap_2()
            .border_color(theme.warning)
            .child(
                div()
                    .font_semibold()
                    .child(tr(bardo, Text::LookImportTitle)),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(tr(bardo, Text::LookImportHint)),
            )
            .child(
                Checkbox::new("persona-look-shows-me")
                    .min_w_0()
                    .checked(shows_me)
                    .label(tr(bardo, Text::LookShowsMe))
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        this.importing = Some(*checked);
                        cx.notify();
                    })),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        Button::new("choose-look-photo")
                            .small()
                            .primary()
                            .label(tr(bardo, Text::LookChoosePhoto))
                            .disabled(!shows_me || self.dialog.is_some())
                            .on_click(
                                cx.listener(|this, _: &ClickEvent, _, cx| this.choose_photo(cx)),
                            ),
                    )
                    .child(
                        Button::new("cancel-look-import")
                            .small()
                            .ghost()
                            .label(tr(bardo, Text::LookCancel))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.importing = None;
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    /// Saves the typed asset id, of the picked kind, for the look.
    fn save_asset(
        &mut self,
        provider: Provider,
        kind: LookAssetKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(persona) = self.persona else {
            return;
        };
        let typed = self.asset_id.read(cx).value().to_string();
        let result = self.bardo.update(cx, |bardo, cx| {
            let result = bardo.set_look_asset(persona, provider, kind, &typed);
            cx.notify();
            result
        });
        match result {
            Ok(_) => {
                self.notice = Some(Notice::AssetSaved);
                self.asset_id
                    .update(cx, |input, cx| input.set_value("", window, cx));
            }
            Err(error) => self.fail(&error, cx),
        }
        self.load(cx);
        cx.notify();
    }

    fn remove_asset(&mut self, provider: Provider, cx: &mut Context<Self>) {
        let Some(persona) = self.persona else {
            return;
        };
        let result = self.bardo.update(cx, |bardo, cx| {
            let result = bardo.remove_look_asset(persona, provider);
            cx.notify();
            result
        });
        match result {
            Ok(()) => self.notice = Some(Notice::AssetRemoved),
            Err(error) => self.fail(&error, cx),
        }
        self.load(cx);
        cx.notify();
    }

    /// Each asset library that can stand for the look: the asset it holds,
    /// or the kind and id to give it.
    fn render_assets(&self, view: &LookView, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(shown) = &self.look else {
            return Vec::new();
        };
        let source = shown.look.source;
        let kinds = LookAssetKind::for_source(source);
        let picked = self
            .asset_kind
            .filter(|kind| kinds.contains(kind))
            .unwrap_or(kinds[0]);
        let mut wells = Vec::new();
        for (provider, held) in view.assets.clone() {
            let bardo = self.bardo.read(cx);
            let muted = cx.theme().muted_foreground;
            let header = h_flex()
                .gap_1()
                .items_center()
                .child(
                    div()
                        .text_sm()
                        .font_medium()
                        .child(tr(bardo, Text::LookAssetTitle)),
                )
                .child(guide::info(
                    bardo,
                    "persona-look-asset-info",
                    tr(bardo, Text::LookAssetHint),
                    guide::refs::PERSONAS_LOOK_ASSET,
                ));
            let body: AnyElement = match held {
                Some(asset) => h_flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(div().text_sm().child(SharedString::from(bardo.text_with(
                        Text::LookAssetHeld,
                        &[
                            ("kind", &bardo.text(Text::LookAssetKindName(asset.kind))),
                            ("id", &asset.id),
                        ],
                    ))))
                    .child(
                        Button::new(("remove-look-asset", provider as usize))
                            .small()
                            .ghost()
                            .label(tr(bardo, Text::LookAssetRemove))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.remove_asset(provider, cx)
                            })),
                    )
                    .into_any_element(),
                None => {
                    let kind_picker = h_flex()
                        .flex_wrap()
                        .items_center()
                        .gap_1()
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(tr(bardo, Text::LookAssetKindLabel)),
                        )
                        .children(kinds.iter().map(|&kind| {
                            Button::new(SharedString::from(format!(
                                "look-asset-kind-{}",
                                kind.code()
                            )))
                            .xsmall()
                            .outline()
                            .selected(kind == picked)
                            .label(tr(bardo, Text::LookAssetKindName(kind)))
                            .on_click(cx.listener(
                                move |this, _: &ClickEvent, _, cx| {
                                    this.asset_kind = Some(kind);
                                    cx.notify();
                                },
                            ))
                        }));
                    v_flex()
                        .gap_2()
                        .child(kind_picker)
                        .child(
                            h_flex()
                                .flex_wrap()
                                .gap_2()
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .min_w(px(200.))
                                        .gap_1()
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(muted)
                                                .child(tr(bardo, Text::LookAssetIdLabel)),
                                        )
                                        .child(Input::new(&self.asset_id).small()),
                                )
                                .child(
                                    h_flex().items_end().child(
                                        Button::new(("save-look-asset", provider as usize))
                                            .small()
                                            .outline()
                                            .label(tr(bardo, Text::LookAssetSave))
                                            .on_click(cx.listener(
                                                move |this, _: &ClickEvent, window, cx| {
                                                    this.save_asset(provider, picked, window, cx)
                                                },
                                            )),
                                    ),
                                ),
                        )
                        .into_any_element()
                }
            };
            wells.push(
                kit::well(cx)
                    .p_3()
                    .gap_2()
                    .child(header)
                    .child(body)
                    .into_any_element(),
            );
        }
        wells
    }

    fn render_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let notice = self.notice?;
        let (tone, text, reason) = match notice {
            Notice::Kept => (Tone::Success, Text::LookKept, None),
            Notice::Imported => (Tone::Success, Text::LookImported, None),
            Notice::Removed => (Tone::Neutral, Text::LookRemoved, None),
            Notice::AssetSaved => (Tone::Success, Text::LookAssetSaved, None),
            Notice::AssetRemoved => (Tone::Neutral, Text::LookAssetRemoved, None),
            Notice::Error(text, reason) => (Tone::Danger, text, reason),
        };
        let reason = reason.map(|reason| {
            let (width, height) = match reason {
                LookImageError::TooSmall { width, height }
                | LookImageError::TooLarge { width, height }
                | LookImageError::Shape { width, height } => (width, height),
                LookImageError::NotAnImage | LookImageError::FileTooLarge(_) => (0, 0),
            };
            let megabytes = match reason {
                LookImageError::FileTooLarge(bytes) => bytes.div_ceil(1_000_000),
                _ => 0,
            };
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(SharedString::from(bardo.text_with(
                    Text::LookImageReason(reason),
                    &[
                        ("width", &width.to_string()),
                        ("height", &height.to_string()),
                        ("mb", &megabytes.to_string()),
                    ],
                )))
        });
        Some(
            v_flex()
                .gap_0p5()
                .child(kit::notice(tone, tr(bardo, text), cx))
                .children(reason)
                .into_any_element(),
        )
    }
}

impl Render for LookSection {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bardo = self.bardo.read(cx);
        let header = h_flex()
            .gap_1()
            .child(
                div()
                    .text_sm()
                    .font_medium()
                    .child(tr(bardo, Text::LookTitle)),
            )
            .child(guide::info(
                bardo,
                "persona-look-info",
                tr(bardo, Text::LookHint),
                guide::refs::PERSONAS_LOOK,
            ));
        let card = kit::card(cx).p_4().gap_3().child(header);
        let Some(view) = self.view.clone() else {
            let text = if self.persona.is_none() {
                Text::LookSaveFirst
            } else if self.load_failed {
                Text::LookNotLoaded
            } else {
                Text::PersonaNotFound
            };
            return card.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr(self.bardo.read(cx), text)),
            );
        };
        let look = self.render_look(cx);
        let assets = self.render_assets(&view, cx);
        let pending = self
            .pending
            .as_ref()
            .map(|shown| self.render_pending(shown, cx));
        let job = self.render_job(&view, cx);
        let draw = self.render_draw(&view, cx);
        let import = self
            .importing
            .map(|shows_me| self.render_import(shows_me, cx));
        let notice = self.render_notice(cx);
        card.child(look)
            .children(assets)
            .children(pending)
            .children(job)
            .child(draw)
            .children(import)
            .children(notice)
    }
}
