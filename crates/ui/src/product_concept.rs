//! The Concept image section of the product form (#280): for a product of
//! kind *idea* with no image of its own, draws a concept image (with its
//! estimate and the budget question), shows the drawing waiting for review
//! with Keep and use / Discard, and the kept one. Rules, storage and the
//! drawing job live in `bardo_app`; this file maps them to the screen.

use std::sync::Arc;
use std::time::Duration;

use bardo_app::bardo_domain::{ImageFormat, JobId, JobState, ProductConcept, ProductId};
use bardo_app::{Bardo, BudgetConsent, ConceptError, ConceptView, SpendEstimate, Text};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, ClickEvent, Entity, Image, ObjectFit, Task, Window, div, img, px};

use crate::guide;
use crate::kit::{self, Tone};
use crate::shell::tr;
use crate::spend::{budget_question, estimate_note};

/// How often a drawing in progress is checked.
const POLL_EVERY: Duration = Duration::from_millis(250);

/// The width of a concept image; its frame is 2:3.
const PICTURE_WIDTH: f32 = 120.;

/// What the section says after the last action.
#[derive(Clone, Copy)]
enum Notice {
    Kept,
    Error(Text),
}

/// A concept image ready to show: the stored one and its decoded picture.
struct Shown {
    concept: ProductConcept,
    picture: Arc<Image>,
}

impl Shown {
    fn of(concept: ProductConcept) -> Self {
        let format = match concept.picture.format() {
            ImageFormat::Png => gpui_kit::ImageFormat::Png,
            ImageFormat::Jpeg => gpui_kit::ImageFormat::Jpeg,
            ImageFormat::Webp => gpui_kit::ImageFormat::Webp,
        };
        let picture = Arc::new(Image::from_bytes(format, concept.picture.bytes().to_vec()));
        Self { concept, picture }
    }
}

pub struct ConceptSection {
    bardo: Entity<Bardo>,
    /// The product being edited; `None` while creating one.
    product: Option<ProductId>,
    /// The section's facts, without the pictures (kept in `kept` and
    /// `pending`, decoded once).
    view: Option<ConceptView>,
    /// The drawing job last seen: the section reloads when it moves.
    seen_job: Option<(JobId, JobState)>,
    /// A finished job whose outcome the user moved past.
    dismissed: Option<JobId>,
    kept: Option<Shown>,
    pending: Option<Shown>,
    /// A drawing past the budget, waiting for the user's answer.
    ask: Option<SpendEstimate>,
    notice: Option<Notice>,
    revision: u64,
    _poll: Task<()>,
}

impl ConceptSection {
    pub fn new(bardo: Entity<Bardo>, cx: &mut Context<Self>) -> Self {
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL_EVERY).await;
                if this.update(cx, |this, cx| this.poll(cx)).is_err() {
                    break;
                }
            }
        });
        let revision = bardo.read(cx).jobs_revision();
        Self {
            bardo,
            product: None,
            view: None,
            seen_job: None,
            dismissed: None,
            kept: None,
            pending: None,
            ask: None,
            notice: None,
            revision,
            _poll: poll,
        }
    }

    /// Shows the concept image of `product` as it is stored now.
    pub fn show(&mut self, product: Option<ProductId>, cx: &mut Context<Self>) {
        if self.product != product {
            self.ask = None;
            self.notice = None;
            self.dismissed = None;
        }
        self.product = product;
        self.load(cx);
        cx.notify();
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let Some(product) = self.product else {
            self.view = None;
            self.kept = None;
            self.pending = None;
            return;
        };
        match self.bardo.read(cx).product_concept(product) {
            Ok(mut view) => {
                let same = |shown: &Option<Shown>, concept: &Option<ProductConcept>| match (
                    shown, concept,
                ) {
                    (Some(shown), Some(concept)) => shown.concept == *concept,
                    (None, None) => true,
                    _ => false,
                };
                if !same(&self.kept, &view.concept) {
                    self.kept = view.concept.clone().map(Shown::of);
                }
                if !same(&self.pending, &view.pending) {
                    self.pending = view.pending.clone().map(Shown::of);
                }
                view.concept = None;
                view.pending = None;
                self.seen_job = view.job.as_ref().map(|job| (job.id(), job.state()));
                self.view = Some(view);
            }
            Err(_) => {
                self.view = None;
                self.kept = None;
                self.pending = None;
            }
        }
    }

    /// Reloads when this product's drawing job moved.
    fn poll(&mut self, cx: &mut Context<Self>) {
        let Some(product) = self.product else {
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
            .product_concept_job(product)
            .map(|job| (job.id(), job.state()));
        if job != self.seen_job {
            self.load(cx);
            cx.notify();
        }
    }

    fn dismiss_job(&mut self) {
        self.dismissed = self
            .view
            .as_ref()
            .and_then(|view| view.job.as_ref())
            .map(|job| job.id());
    }

    fn fail(&mut self, error: &ConceptError, cx: &mut Context<Self>) {
        self.notice = Some(Notice::Error(error.message()));
        cx.notify();
    }

    fn draw(&mut self, consent: BudgetConsent, cx: &mut Context<Self>) {
        let Some(product) = self.product else {
            return;
        };
        self.notice = None;
        self.ask = None;
        match self.bardo.read(cx).draw_product_concept(product, consent) {
            Ok(_) => self.load(cx),
            Err(ConceptError::OverBudget(estimate)) => self.ask = Some(estimate),
            Err(error) => self.fail(&error, cx),
        }
        cx.notify();
    }

    fn keep(&mut self, cx: &mut Context<Self>) {
        let Some(product) = self.product else {
            return;
        };
        match self.bardo.read(cx).keep_product_concept(product) {
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
        let Some(product) = self.product else {
            return;
        };
        self.notice = None;
        match self.bardo.read(cx).discard_product_concept(product) {
            Ok(()) => self.dismiss_job(),
            Err(error) => self.fail(&error, cx),
        }
        self.load(cx);
        cx.notify();
    }

    /// A concept image in its 2:3 frame.
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

    fn muted(text: impl IntoElement, cx: &App) -> AnyElement {
        div()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(text)
            .into_any_element()
    }

    /// The kept concept image, and whether the product's ads use it.
    fn render_kept(&self, view: &ConceptView, cx: &mut Context<Self>) -> Option<AnyElement> {
        let shown = self.kept.as_ref()?;
        let bardo = self.bardo.read(cx);
        let note = if view.offered {
            Text::ConceptKeptNote
        } else {
            Text::ConceptUnusedNote
        };
        Some(
            h_flex()
                .flex_wrap()
                .items_start()
                .gap_3()
                .child(Self::picture(shown, cx))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w(px(180.))
                        .gap_2()
                        .child(h_flex().child(kit::status(
                            Tone::Accent,
                            tr(
                                bardo,
                                Text::ProductImageKindName(shown.concept.as_image().kind),
                            ),
                            cx,
                        )))
                        .child(Self::muted(tr(bardo, note), cx)),
                )
                .into_any_element(),
        )
    }

    /// The drawing waiting for review, with Keep and use and Discard.
    fn render_pending(&self, shown: &Shown, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
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
                            .min_w(px(180.))
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .font_medium()
                                    .child(tr(bardo, Text::ConceptNewTitle)),
                            )
                            .child(
                                h_flex()
                                    .flex_wrap()
                                    .gap_2()
                                    .child(
                                        Button::new("keep-concept")
                                            .small()
                                            .primary()
                                            .label(tr(bardo, Text::KeepConcept))
                                            .on_click(cx.listener(
                                                |this, _: &ClickEvent, _, cx| this.keep(cx),
                                            )),
                                    )
                                    .child(
                                        Button::new("discard-concept")
                                            .small()
                                            .ghost()
                                            .label(tr(bardo, Text::DiscardConcept))
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
    fn render_job(&self, view: &ConceptView, cx: &mut Context<Self>) -> Option<AnyElement> {
        let bardo = self.bardo.read(cx);
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
                        .child(Self::muted(tr(bardo, Text::ConceptDrawingNow), cx))
                        .into_any_element(),
                );
            }
            JobState::Done => return None,
            JobState::Cancelled => kit::notice(Tone::Neutral, tr(bardo, Text::ConceptStopped), cx),
            JobState::Failed => kit::notice(Tone::Danger, tr(bardo, Text::ConceptFailed), cx),
        };
        Some(line.into_any_element())
    }

    fn render_draw(&self, view: &ConceptView, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !view.offered {
            return None;
        }
        let bardo = self.bardo.read(cx);
        let drawing = view.is_drawing();
        let again = self.kept.is_some() || self.pending.is_some();
        let ask = self.ask.as_ref().map(|estimate| {
            budget_question(
                "product-concept-budget",
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
        Some(
            v_flex()
                .gap_2()
                .children(estimate_note(bardo, &view.estimate, Text::EstimateCost, cx))
                .child(
                    h_flex().child(
                        Button::new("draw-concept")
                            .small()
                            .outline()
                            .label(tr(
                                bardo,
                                if again {
                                    Text::DrawConceptAgain
                                } else {
                                    Text::DrawConcept
                                },
                            ))
                            .loading(drawing)
                            .disabled(drawing || self.ask.is_some())
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.draw(BudgetConsent::Ask, cx)
                            })),
                    ),
                )
                .children(ask)
                .into_any_element(),
        )
    }

    fn render_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let bardo = self.bardo.read(cx);
        let (tone, text) = match self.notice? {
            Notice::Kept => (Tone::Success, Text::ConceptKept),
            Notice::Error(text) => (Tone::Danger, text),
        };
        Some(kit::notice(tone, tr(bardo, text), cx).into_any_element())
    }
}

impl Render for ConceptSection {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(view) = self.view.clone() else {
            return div().into_any_element();
        };
        // Only an idea with no image of its own is offered one; one kept
        // or waiting stays in view.
        if !view.offered && self.kept.is_none() && self.pending.is_none() && !view.is_drawing() {
            return div().into_any_element();
        }
        let bardo = self.bardo.read(cx);
        let header = v_flex().gap_1().child(
            h_flex()
                .gap_1()
                .items_center()
                .child(kit::section_heading(tr(bardo, Text::ConceptTitle)))
                .child(guide::info(
                    bardo,
                    "product-concept-info",
                    tr(bardo, Text::ConceptHint),
                    guide::refs::PRODUCTS_CONCEPT,
                )),
        );
        let kept = self.render_kept(&view, cx);
        let pending = self
            .pending
            .as_ref()
            .map(|shown| self.render_pending(shown, cx));
        let job = self.render_job(&view, cx);
        let draw = self.render_draw(&view, cx);
        let notice = self.render_notice(cx);
        kit::card(cx)
            .p_4()
            .gap_3()
            .child(header)
            .children(kept)
            .children(pending)
            .children(job)
            .children(draw)
            .children(notice)
            .into_any_element()
    }
}
