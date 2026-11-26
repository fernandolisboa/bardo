//! The export for ads on the Publish stage of an ad (#283): the texts an
//! ad manager asks for, the shapes to render (4:5, 1:1 and the editor's
//! other shape) with where each stands, the window each clip shows
//! through in a shape, and the export itself with its progress.
//!
//! It goes with the items acknowledged in the ad's review card, like the
//! export for networks. A shape not exported yet, or out of date, is
//! chosen until the user unticks it.

use std::path::PathBuf;

use bardo_app::bardo_domain::{
    AD_HEADLINE_MAX, AD_PRIMARY_MAX, AdTexts, CROP_STEPS, CropPosition, FrameShape, JobState,
    VideoProjectId, crop_window,
};
use bardo_app::{
    Bardo, PlacementClip, PlacementError, PlacementReview, PlacementShape, Text,
    placement_job_shapes,
};
use gpui_kit::component::button::{Button, ButtonGroup, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::{
    Disableable as _, Selectable as _, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Entity, ObjectFit, SharedString, Window, div, img, px, relative,
};

use super::{ProjectsScreen, muted};
use crate::appearance::look;
use crate::guide;
use crate::kit::{self, Tone};
use crate::shell::tr;

/// How far one press moves a window, in steps of its range.
const NUDGE: u16 = CROP_STEPS / 10;

/// The export for ads as the Publish stage holds it.
pub(super) struct Placements {
    review: Option<PlacementReview>,
    /// What failed: reading the export, saving the texts or starting it.
    error: Option<Text>,
    /// The shapes the user ticked or unticked; the others follow whether
    /// they are current.
    choices: Vec<(FrameShape, bool)>,
    /// The shape whose windows show.
    shown: Option<FrameShape>,
    primary: Entity<TextareaState>,
    headline: Entity<InputState>,
    /// The stored texts last put in the fields.
    loaded: Option<(VideoProjectId, AdTexts)>,
}

impl Placements {
    pub(super) fn new(window: &mut Window, cx: &mut Context<ProjectsScreen>) -> Self {
        Placements {
            review: None,
            error: None,
            choices: Vec::new(),
            shown: None,
            primary: cx.new(|cx| TextareaState::new(window, cx).auto_grow(2, 5)),
            headline: cx.new(|cx| InputState::new(window, cx)),
            loaded: None,
        }
    }

    /// Forgets the last project's export.
    pub(super) fn reset(&mut self) {
        self.review = None;
        self.error = None;
        self.choices.clear();
        self.shown = None;
        self.loaded = None;
    }

    /// The texts as the user typed them.
    fn typed(&self, cx: &App) -> AdTexts {
        AdTexts::new(
            &self.primary.read(cx).value(),
            &self.headline.read(cx).value(),
        )
    }

    /// Whether the fields differ from the stored texts.
    fn dirty(&self, cx: &App) -> bool {
        self.loaded
            .as_ref()
            .is_some_and(|(_, stored)| *stored != self.typed(cx))
    }

    fn chosen(&self, shape: &PlacementShape) -> bool {
        self.choices
            .iter()
            .rev()
            .find(|(picked, _)| *picked == shape.shape)
            .map_or(!shape.current, |(_, chosen)| *chosen)
    }

    fn chosen_shapes(&self) -> Vec<FrameShape> {
        self.review.as_ref().map_or_else(Vec::new, |review| {
            review
                .shapes
                .iter()
                .filter(|shape| self.chosen(shape))
                .map(|shape| shape.shape)
                .collect()
        })
    }
}

/// Whether `job` is queued or running.
fn running(job: Option<&bardo_app::bardo_domain::Job>) -> bool {
    job.is_some_and(|job| job.state().is_active())
}

/// A field's label.
fn label(text: SharedString) -> gpui_kit::Div {
    div().text_xs().font_semibold().child(text)
}

/// A small heading inside the card.
fn heading(bardo: &Bardo, text: Text) -> gpui_kit::Div {
    div().text_sm().font_medium().child(tr(bardo, text))
}

impl ProjectsScreen {
    /// Reads the export for ads of `id`, an ad, and fills the texts'
    /// fields when the stored ones changed.
    pub(super) fn load_placements(
        &mut self,
        id: VideoProjectId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let is_ad = self
            .export_view
            .as_ref()
            .is_some_and(|view| view.ad.is_some());
        if !is_ad {
            self.placements.review = None;
            return;
        }
        match self.bardo.read(cx).placement_review(id) {
            Ok(review) => self.placements.review = review,
            // Nothing to export before there is a cut.
            Err(PlacementError::NoCut) => self.placements.review = None,
            Err(error) => {
                self.placements.review = None;
                self.placements.error = Some(error.message());
            }
        }
        let stored = self
            .placements
            .review
            .as_ref()
            .and_then(|review| review.texts.clone())
            .map(|texts| (id, texts));
        if stored.is_some() && self.placements.loaded != stored {
            let (primary, headline) = stored
                .as_ref()
                .map(|(_, texts)| (texts.primary().to_owned(), texts.headline().to_owned()))
                .unwrap_or_default();
            self.placements
                .primary
                .update(cx, |input, cx| input.set_value(primary, window, cx));
            self.placements
                .headline
                .update(cx, |input, cx| input.set_value(headline, window, cx));
            self.placements.loaded = stored;
        }
    }

    fn save_ad_texts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.project else {
            return;
        };
        let typed = self.placements.typed(cx);
        self.placements.error = self
            .bardo
            .read(cx)
            .set_ad_texts(id, typed.primary(), typed.headline())
            .err()
            .map(|error| error.message());
        self.load(window, cx);
        cx.notify();
    }

    fn choose_placement(&mut self, shape: FrameShape, chosen: bool, cx: &mut Context<Self>) {
        self.placements.choices.push((shape, chosen));
        cx.notify();
    }

    /// Moves a clip's window in `shape` to `position`.
    fn move_window(
        &mut self,
        shape: FrameShape,
        clip: String,
        position: CropPosition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.project else {
            return;
        };
        self.placements.error = self
            .bardo
            .read(cx)
            .set_placement_window(id, shape, &clip, position)
            .err()
            .map(|error| error.message());
        self.load(window, cx);
        cx.notify();
    }

    fn start_placements(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(review) = self.placements.review.as_ref() else {
            return;
        };
        let chosen = self.placements.chosen_shapes();
        match self
            .bardo
            .read(cx)
            .start_placement_export(review, &chosen, &self.ad_acknowledged)
        {
            Ok(_) => {
                self.placements.choices.clear();
                self.placements.error = None;
            }
            Err(error) => self.placements.error = Some(error.message()),
        }
        self.load(window, cx);
        cx.notify();
    }

    fn cancel_placements(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(job) = self
            .placements
            .review
            .as_ref()
            .and_then(|review| review.job.as_ref())
        else {
            return;
        };
        if self.bardo.read(cx).cancel_job(job.id()).is_err() {
            self.placements.error = Some(Text::ExportNotLoaded);
        }
        self.load(window, cx);
        cx.notify();
    }

    /// The texts for an ad manager: the two fields with their counters,
    /// what keeps them from going out, and Save.
    fn placement_texts(&self, review: &PlacementReview, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let title = h_flex()
            .gap_1()
            .items_center()
            .child(heading(bardo, Text::PlacementTextsTitle))
            .child(guide::info(
                bardo,
                "placement-texts-info",
                tr(bardo, Text::PlacementTextsHint),
                guide::refs::EXPORTING_ADS,
            ));
        let mut column = v_flex().gap_2().child(title);
        if review.texts.is_none() {
            return column
                .child(muted(cx, tr(bardo, Text::PlacementTextsNone)))
                .into_any_element();
        }
        let typed = self.placements.typed(cx);
        let counter = |n: usize, max: usize| {
            let over = n > max;
            div()
                .text_xs()
                .text_color(if over {
                    look(cx).tokens.danger
                } else {
                    look(cx).tokens.text2
                })
                .child(SharedString::from(bardo.text_with(
                    Text::PlacementCount,
                    &[("n", &n.to_string()), ("max", &max.to_string())],
                )))
        };
        column = column
            .child(
                v_flex()
                    .gap_1()
                    .child(label(tr(bardo, Text::PlacementPrimary)))
                    .child(Textarea::new(&self.placements.primary))
                    .child(counter(typed.primary().chars().count(), AD_PRIMARY_MAX)),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(label(tr(bardo, Text::PlacementHeadline)))
                    .child(Input::new(&self.placements.headline).small())
                    .child(counter(typed.headline().chars().count(), AD_HEADLINE_MAX)),
            );
        column = column.children(typed.problems().into_iter().map(|problem| {
            kit::notice(
                Tone::Warning,
                SharedString::from(bardo.ad_text_problem_text(problem)),
                cx,
            )
            .text_xs()
        }));
        if self.placements.dirty(cx) {
            column = column.child(
                h_flex().child(
                    Button::new("placement-texts-save")
                        .small()
                        .primary()
                        .label(tr(bardo, Text::PlacementTextsSave))
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.save_ad_texts(window, cx);
                        })),
                ),
            );
        }
        column.into_any_element()
    }

    /// The shapes: a box each, where it stands, and its folder.
    fn placement_shapes(
        &self,
        review: &PlacementReview,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let screen = cx.entity().downgrade();
        let rows = review.shapes.iter().enumerate().map(|(i, shape)| {
            let (tone, state) = match (&shape.last, shape.current) {
                (Some(_), true) => (Tone::Success, Text::PlacementCurrent),
                (Some(_), false) => (Tone::Warning, Text::PlacementOutdated),
                (None, _) => (Tone::Neutral, Text::PlacementNever),
            };
            let which = shape.shape;
            let screen = screen.clone();
            let file = shape
                .last
                .as_ref()
                .map(|last| bardo.placement_folder(review, which).join(&last.file))
                .filter(|file| file.exists());
            h_flex()
                .gap_2()
                .flex_wrap()
                .items_center()
                .child(
                    Checkbox::new(("placement-shape", i))
                        .label(SharedString::from(bardo.placement_shape_text(which)))
                        .checked(self.placements.chosen(shape))
                        .disabled(busy)
                        .on_click(move |checked: &bool, _, cx| {
                            let checked = *checked;
                            let _ = screen
                                .update(cx, |this, cx| this.choose_placement(which, checked, cx));
                        }),
                )
                .child(kit::status(tone, tr(bardo, state), cx))
                .children(file.map(|file: PathBuf| {
                    Button::new(("placement-folder", i))
                        .small()
                        .ghost()
                        .label(tr(bardo, Text::PlacementShowFolder))
                        .on_click(move |_, _, cx| cx.reveal_path(&file))
                }))
        });
        v_flex().gap_1().children(rows).into_any_element()
    }

    /// One clip's window in `shape`: its picture dimmed, the window
    /// outlined over it, and the buttons that move it.
    fn placement_clip(
        &self,
        shape: FrameShape,
        clip: &PlacementClip,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let t = look(cx).tokens;
        let rect = crop_window(clip.source, shape, clip.position);
        let (width, height) = (clip.source.width.max(1), clip.source.height.max(1));
        let share = |part: u32, whole: u32| part as f32 / whole as f32;
        let mut frame = div()
            .relative()
            .w_full()
            .aspect_ratio(width as f32 / height as f32)
            .rounded(t.radius)
            .overflow_hidden()
            .bg(t.sunken);
        if let Some(path) = clip.thumbnail.clone() {
            frame = frame.child(
                img(path)
                    .absolute()
                    .inset_0()
                    .size_full()
                    .object_fit(ObjectFit::Fill)
                    .opacity(0.45),
            );
        }
        frame = frame.child(
            div()
                .absolute()
                .left(relative(share(rect.x, width)))
                .top(relative(share(rect.y, height)))
                .w(relative(share(rect.width, width)))
                .h(relative(share(rect.height, height)))
                .border_2()
                .border_color(t.accent),
        );
        let name = match clip.scene {
            Some(scene) => {
                bardo.text_with(Text::PlacementScene, &[("n", &(scene + 1).to_string())])
            }
            None => bardo.text(Text::PlacementFootage).into_owned(),
        };
        let (x, y) = (clip.position.x(), clip.position.y());
        let moves = [
            ("←", Text::PlacementLeft, x.saturating_sub(NUDGE), y),
            ("→", Text::PlacementRight, x.saturating_add(NUDGE), y),
            ("↑", Text::PlacementUp, x, y.saturating_sub(NUDGE)),
            ("↓", Text::PlacementDown, x, y.saturating_add(NUDGE)),
        ];
        let id = SharedString::from(format!("placement-{}-{}", shape.folder(), clip.index));
        let buttons = moves
            .into_iter()
            .enumerate()
            .map(|(i, (arrow, text, x, y))| {
                let position = CropPosition::new(x, y);
                let key = clip.key.clone();
                Button::new((id.clone(), i))
                    .xsmall()
                    .outline()
                    .label(arrow)
                    .tooltip(tr(bardo, text))
                    .disabled(busy || position == clip.position)
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.move_window(shape, key.clone(), position, window, cx);
                    }))
            });
        let center = {
            let key = clip.key.clone();
            Button::new((id.clone(), usize::MAX))
                .xsmall()
                .ghost()
                .label(tr(bardo, Text::PlacementCenter))
                .disabled(busy || !clip.moved)
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.move_window(shape, key.clone(), CropPosition::CENTER, window, cx);
                }))
        };
        v_flex()
            .w(px(168.))
            .gap_1()
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        div()
                            .text_xs()
                            .font_medium()
                            .child(SharedString::from(name)),
                    )
                    .children(
                        clip.moved
                            .then(|| kit::status(Tone::Info, tr(bardo, Text::PlacementMoved), cx)),
                    ),
            )
            .child(frame)
            .child(
                h_flex()
                    .gap_1()
                    .flex_wrap()
                    .items_center()
                    .children(buttons)
                    .child(center),
            )
            .into_any_element()
    }

    /// The windows of the shown shape, with the shape to show picked above.
    fn placement_windows(
        &self,
        review: &PlacementReview,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let shown = self
            .placements
            .shown
            .filter(|shown| review.shapes.iter().any(|shape| shape.shape == *shown))
            .or_else(|| review.shapes.first().map(|shape| shape.shape));
        let bardo = self.bardo.read(cx);
        let title = h_flex()
            .gap_1()
            .items_center()
            .child(heading(bardo, Text::PlacementWindowsTitle))
            .child(guide::info(
                bardo,
                "placement-windows-info",
                tr(bardo, Text::PlacementWindowsHint),
                guide::refs::EXPORTING_ADS,
            ));
        let codes: Vec<FrameShape> = review.shapes.iter().map(|shape| shape.shape).collect();
        let picker = ButtonGroup::new("placement-shown")
            .outline()
            .small()
            .children(codes.iter().map(|shape| {
                Button::new(shape.folder())
                    .label(shape.code())
                    .selected(Some(*shape) == shown)
            }))
            .on_click(cx.listener(move |this, clicked: &Vec<usize>, _, cx| {
                if let Some(shape) = clicked.first().and_then(|&i| codes.get(i)) {
                    this.placements.shown = Some(*shape);
                    cx.notify();
                }
            }));
        let mut column = v_flex().gap_2().child(title).child(h_flex().child(picker));
        let Some(shape) = shown.and_then(|shown| {
            review
                .shapes
                .iter()
                .find(|shape| shape.shape == shown)
                .cloned()
        }) else {
            return column.into_any_element();
        };
        if !shape.windows {
            column = column.child(muted(cx, tr(bardo, Text::PlacementWholeWithBars)));
        } else {
            let clips: Vec<AnyElement> = shape
                .clips
                .iter()
                .map(|clip| self.placement_clip(shape.shape, clip, busy, cx))
                .collect();
            column = column.child(h_flex().gap_3().flex_wrap().items_start().children(clips));
        }
        column.into_any_element()
    }

    /// The export itself: what keeps it from starting, its progress, and
    /// the button.
    fn placement_actions(&self, review: &PlacementReview, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let mut column = v_flex().gap_2();
        if !review.unchecked_products.is_empty() {
            column = column.child(
                kit::notice(Tone::Warning, tr(bardo, Text::RenderProductUnchecked), cx).text_xs(),
            );
        }
        if review.pictures_wait {
            column = column.child(
                kit::notice(Tone::Warning, tr(bardo, Text::PlacementPicturesWait), cx).text_xs(),
            );
        }
        if let Some(error) = self.placements.error {
            column = column.child(kit::notice(Tone::Danger, tr(bardo, error), cx).text_xs());
        }
        if let Some(job) = review.job.as_ref().filter(|job| job.state().is_active()) {
            let (done, total) = placement_job_shapes(job);
            return column
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(div().text_sm().child(SharedString::from(bardo.text_with(
                            Text::PlacementRunning,
                            &[
                                ("done", &(done + 1).min(total.max(1)).to_string()),
                                ("total", &total.to_string()),
                            ],
                        ))))
                        .child(
                            div().w(px(160.)).child(
                                Progress::new("placement-progress")
                                    .small()
                                    .value(job.progress().permille() as f32 / 10.0),
                            ),
                        )
                        .child(
                            Button::new("placement-cancel")
                                .small()
                                .outline()
                                .label(tr(bardo, Text::CancelJob))
                                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                    this.cancel_placements(window, cx);
                                })),
                        ),
                )
                .into_any_element();
        }
        if review
            .job
            .as_ref()
            .is_some_and(|job| matches!(job.state(), JobState::Failed | JobState::Cancelled))
        {
            column = column
                .child(kit::notice(Tone::Warning, tr(bardo, Text::PlacementFailed), cx).text_xs());
        }
        let chosen = self.placements.chosen_shapes().len();
        let left = self.ad_left(&review.publishing);
        let unsaved = self.placements.dirty(cx);
        let mut row = h_flex().gap_2().flex_wrap().items_center();
        if left > 0 {
            row = row.child(muted(cx, tr(bardo, Text::AdNotAcknowledged)));
        }
        row = row.child(
            Button::new("placement-start")
                .small()
                .primary()
                .disabled(!review.ready() || chosen == 0 || left > 0 || unsaved)
                .label(tr(bardo, Text::PlacementStart))
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.start_placements(window, cx);
                })),
        );
        column.child(row).into_any_element()
    }

    /// The Publish stage's card of an ad's export for ads.
    pub(super) fn placement_card(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let review = self.placements.review.clone()?;
        let busy = running(review.job.as_ref());
        let texts = self.placement_texts(&review, cx);
        let shapes = self.placement_shapes(&review, busy, cx);
        let windows = self.placement_windows(&review, busy, cx);
        let actions = self.placement_actions(&review, cx);
        let bardo = self.bardo.read(cx);
        Some(
            kit::card(cx)
                .p_3()
                .gap_3()
                .child(
                    h_flex()
                        .gap_1()
                        .items_center()
                        .child(div().font_semibold().child(tr(bardo, Text::PlacementTitle)))
                        .child(guide::info(
                            bardo,
                            "placement-info",
                            tr(bardo, Text::PlacementHint),
                            guide::refs::EXPORTING_ADS,
                        )),
                )
                .child(texts)
                .child(shapes)
                .child(windows)
                .child(actions)
                .into_any_element(),
        )
    }
}
