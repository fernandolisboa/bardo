//! The Sheet tab of a product: what Bardo knows about it beyond the
//! brief, for the user to review. It starts a research on the web or a
//! sheet written from the brief (each with its estimate and the budget
//! question), or an empty sheet filled by hand; edits the parts that
//! describe the product; and lists the claims, where the user confirms or
//! rejects what a model wrote, edits a claim, marks it sensitive and adds
//! claims of their own. Rules, storage and the job live in `bardo_app`;
//! this file maps them to the screen.

use std::time::Duration;

use bardo_app::bardo_domain::{
    Alternative, AudienceSegment, Claim, ClaimId, ClaimKind, ClaimSource, ClaimState,
    JobFailureKind, JobId, JobState, Objection, ProductId, ProductSheet, ResearchLimits,
    RestrictedCategory, SheetFieldError, SheetText,
};
use bardo_app::{
    Bardo, BudgetConsent, Control, SheetError, SheetRun, SheetView, SpendEstimate, Text, TourAnchor,
};
use gpui_kit::component::button::{Button, ButtonGroup, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Selectable as _, Sizable as _, StyledExt as _, h_flex,
    v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Entity, ScrollHandle, SharedString, Subscription, Task, Window,
    div, px,
};

use crate::guide;
use crate::kit::{self, Tone};
use crate::shell::tr;
use crate::spend::{budget_question, estimate_note};

/// How often a run in progress is checked.
const POLL_EVERY: Duration = Duration::from_millis(250);

/// What the tab says after the last action.
#[derive(Clone, Copy)]
enum Notice {
    Saved,
    Error(Text),
}

/// A list of the sheet edited row by row.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Part {
    Segments,
    Objections,
    Alternatives,
}

impl Part {
    const ALL: [Part; 3] = [Part::Segments, Part::Objections, Part::Alternatives];

    fn index(self) -> usize {
        self as usize
    }

    fn title(self) -> Text {
        match self {
            Part::Segments => Text::SheetSegmentsTitle,
            Part::Objections => Text::SheetObjectionsTitle,
            Part::Alternatives => Text::SheetAlternativesTitle,
        }
    }

    fn add(self) -> Text {
        match self {
            Part::Segments => Text::SheetAddSegment,
            Part::Objections => Text::SheetAddObjection,
            Part::Alternatives => Text::SheetAddAlternative,
        }
    }

    /// What each field of a row holds, shown as its placeholder.
    fn fields(self) -> &'static [Text] {
        match self {
            Part::Segments => &[
                Text::SheetSegmentName,
                Text::SheetSegmentWants,
                Text::SheetSegmentHoldsBack,
            ],
            Part::Objections => &[Text::SheetObjection, Text::SheetObjectionAnswer],
            Part::Alternatives => &[Text::SheetAlternativeName, Text::SheetAlternativeDiffers],
        }
    }

    /// The most rows the sheet holds.
    fn limit(self) -> usize {
        match self {
            Part::Segments => SheetText::MAX_SEGMENTS,
            Part::Objections => SheetText::MAX_OBJECTIONS,
            Part::Alternatives => SheetText::MAX_ALTERNATIVES,
        }
    }

    fn errors(self) -> &'static [SheetFieldError] {
        match self {
            Part::Segments => &[SheetFieldError::TooManySegments, SheetFieldError::Segment],
            Part::Objections => &[
                SheetFieldError::TooManyObjections,
                SheetFieldError::Objection,
            ],
            Part::Alternatives => &[
                SheetFieldError::TooManyAlternatives,
                SheetFieldError::Alternative,
            ],
        }
    }

    fn id(self) -> &'static str {
        match self {
            Part::Segments => "sheet-segment",
            Part::Objections => "sheet-objection",
            Part::Alternatives => "sheet-alternative",
        }
    }
}

/// A claim being edited: its text and kind as typed so far.
struct ClaimEdit {
    claim: ClaimId,
    text: Entity<InputState>,
    kind: ClaimKind,
}

pub struct SheetSection {
    bardo: Entity<Bardo>,
    /// The scroll of the form the tab sits in, so a tour finds its parts.
    scroll: ScrollHandle,
    /// The product being edited; `None` while creating one.
    product: Option<ProductId>,
    view: Option<SheetView>,
    load_failed: bool,
    /// The run last seen, by id, state and how far it is: the tab
    /// reloads when it moves, from the research to the writing too.
    seen_job: Option<(JobId, JobState, u16)>,
    summary: Entity<TextareaState>,
    category: Entity<InputState>,
    restricted: Option<RestrictedCategory>,
    looks: Entity<TextareaState>,
    /// The rows of each [`Part`], one input per field.
    rows: [Vec<Vec<Entity<InputState>>>; 3],
    /// The form shows what the stored sheet says again at the next
    /// render, which has the window the inputs need.
    refill: bool,
    field_errors: Vec<SheetFieldError>,
    new_claim: Entity<InputState>,
    new_kind: ClaimKind,
    editing: Option<ClaimEdit>,
    /// A run past the budget, waiting for the user's answer.
    ask: Option<(SheetRun, SpendEstimate)>,
    notice: Option<Notice>,
    /// Why a run did not start, shown where it was asked for.
    fill_error: Option<Text>,
    /// What went wrong with a claim, shown in the claims card.
    claim_error: Option<Text>,
    revision: u64,
    _poll: Task<()>,
    _subscriptions: Vec<Subscription>,
}

/// A run as the tab compares it from one look to the next.
fn seen(job: &bardo_app::bardo_domain::Job) -> (JobId, JobState, u16) {
    (job.id(), job.state(), job.progress().permille())
}

impl SheetSection {
    pub fn new(
        bardo: Entity<Bardo>,
        scroll: ScrollHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let summary = cx.new(|cx| TextareaState::new(window, cx).auto_grow(3, 10));
        let category = cx.new(|cx| InputState::new(window, cx));
        let looks = cx.new(|cx| TextareaState::new(window, cx).auto_grow(2, 8));
        let new_claim = cx.new(|cx| InputState::new(window, cx));
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL_EVERY).await;
                if this.update(cx, |this, cx| this.poll(cx)).is_err() {
                    break;
                }
            }
        });
        let subscriptions = vec![
            cx.subscribe(&summary, |this, _, event, cx| this.typed(event, cx)),
            cx.subscribe(&category, |this, _, event, cx| this.typed(event, cx)),
            cx.subscribe(&looks, |this, _, event, cx| this.typed(event, cx)),
            cx.subscribe(&new_claim, |this, _, event, cx| {
                if matches!(event, InputEvent::Change) && this.claim_error.is_some() {
                    this.claim_error = None;
                    cx.notify();
                }
            }),
            cx.observe_in(&bardo, window, |this, _, window, cx| {
                this.relabel(window, cx)
            }),
        ];
        let revision = bardo.read(cx).jobs_revision();
        let mut section = Self {
            bardo,
            scroll,
            product: None,
            view: None,
            load_failed: false,
            seen_job: None,
            summary,
            category,
            restricted: None,
            looks,
            rows: [Vec::new(), Vec::new(), Vec::new()],
            refill: false,
            field_errors: Vec::new(),
            new_claim,
            new_kind: ClaimKind::Benefit,
            editing: None,
            ask: None,
            notice: None,
            fill_error: None,
            claim_error: None,
            revision,
            _poll: poll,
            _subscriptions: subscriptions,
        };
        section.relabel(window, cx);
        section
    }

    /// Shows the sheet of `product` as it is stored, or the note to save
    /// the product first. Edits of the same product's sheet not saved yet
    /// stay in the form.
    pub fn show(
        &mut self,
        product: Option<ProductId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keep = self.product == product && self.dirty(cx);
        if self.product != product {
            self.product = product;
            self.ask = None;
            self.notice = None;
            self.fill_error = None;
            self.claim_error = None;
            self.editing = None;
            self.field_errors.clear();
            self.new_claim
                .update(cx, |input, cx| input.set_value("", window, cx));
        }
        self.load(cx);
        if !keep {
            self.fill(window, cx);
        }
        cx.notify();
    }

    /// Whether a run is filling the sheet.
    fn busy(&self) -> bool {
        self.view.as_ref().is_some_and(SheetView::is_busy)
    }

    fn sheet(&self) -> Option<&ProductSheet> {
        self.view.as_ref().and_then(|view| view.sheet.as_ref())
    }

    fn typed(&mut self, event: &InputEvent, cx: &mut Context<Self>) {
        if matches!(event, InputEvent::Change)
            && (self.notice.is_some() || !self.field_errors.is_empty())
        {
            self.notice = None;
            self.field_errors.clear();
            cx.notify();
        }
    }

    fn relabel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let placeholder = tr(self.bardo.read(cx), Text::AddClaimPlaceholder);
        self.new_claim.update(cx, |input, cx| {
            input.set_placeholder(placeholder, window, cx)
        });
        for part in Part::ALL {
            let labels: Vec<SharedString> = part
                .fields()
                .iter()
                .map(|text| tr(self.bardo.read(cx), *text))
                .collect();
            for row in &self.rows[part.index()] {
                for (input, label) in row.iter().zip(&labels) {
                    input.update(cx, |input, cx| {
                        input.set_placeholder(label.clone(), window, cx)
                    });
                }
            }
        }
        cx.notify();
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let Some(product) = self.product else {
            self.view = None;
            self.load_failed = false;
            self.seen_job = None;
            return;
        };
        match self.bardo.read(cx).product_sheet(product) {
            Ok(view) => {
                self.seen_job = view.job.as_ref().map(|(job, _)| seen(job));
                self.view = Some(view);
                self.load_failed = false;
            }
            Err(_) => {
                self.view = None;
                self.seen_job = None;
                self.load_failed = true;
            }
        }
    }

    /// A new row of `part` holding `values`.
    fn row(
        &self,
        part: Part,
        values: &[&str],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Entity<InputState>> {
        part.fields()
            .iter()
            .enumerate()
            .map(|(ix, label)| {
                let placeholder = tr(self.bardo.read(cx), *label);
                let value = values.get(ix).copied().unwrap_or_default().to_owned();
                cx.new(|cx| {
                    let mut input = InputState::new(window, cx);
                    input.set_placeholder(placeholder, window, cx);
                    input.set_value(value, window, cx);
                    input
                })
            })
            .collect()
    }

    /// Fills the form from the stored sheet.
    fn fill(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.refill = false;
        let text = self.sheet().map(|sheet| sheet.text().clone());
        let text = text.unwrap_or_default();
        self.summary.update(cx, |input, cx| {
            input.set_value(text.summary.clone(), window, cx)
        });
        self.category.update(cx, |input, cx| {
            input.set_value(text.category.clone(), window, cx)
        });
        self.looks.update(cx, |input, cx| {
            input.set_value(text.looks.clone(), window, cx)
        });
        self.restricted = text.restricted;
        let segments = text
            .segments
            .iter()
            .map(|s| {
                self.row(
                    Part::Segments,
                    &[&s.name, &s.wants, &s.holds_back],
                    window,
                    cx,
                )
            })
            .collect();
        let objections = text
            .objections
            .iter()
            .map(|o| self.row(Part::Objections, &[&o.objection, &o.answer], window, cx))
            .collect();
        let alternatives = text
            .alternatives
            .iter()
            .map(|a| self.row(Part::Alternatives, &[&a.name, &a.differs], window, cx))
            .collect();
        self.rows = [segments, objections, alternatives];
        self.field_errors.clear();
    }

    /// The sheet's text as the form holds it. A row left wholly empty is
    /// no row.
    fn text(&self, cx: &App) -> SheetText {
        let rows = |part: Part| -> Vec<Vec<String>> {
            self.rows[part.index()]
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|input| input.read(cx).value().trim().to_owned())
                        .collect::<Vec<String>>()
                })
                .filter(|row| row.iter().any(|value| !value.is_empty()))
                .collect()
        };
        let cell = |row: &[String], ix: usize| row.get(ix).cloned().unwrap_or_default();
        SheetText {
            summary: self.summary.read(cx).value().to_string(),
            category: self.category.read(cx).value().to_string(),
            restricted: self.restricted,
            segments: rows(Part::Segments)
                .iter()
                .map(|row| AudienceSegment {
                    name: cell(row, 0),
                    wants: cell(row, 1),
                    holds_back: cell(row, 2),
                })
                .collect(),
            objections: rows(Part::Objections)
                .iter()
                .map(|row| Objection {
                    objection: cell(row, 0),
                    answer: cell(row, 1),
                })
                .collect(),
            alternatives: rows(Part::Alternatives)
                .iter()
                .map(|row| Alternative {
                    name: cell(row, 0),
                    differs: cell(row, 1),
                })
                .collect(),
            looks: self.looks.read(cx).value().to_string(),
        }
    }

    /// Whether the form differs from the stored sheet's text.
    fn dirty(&self, cx: &App) -> bool {
        match self.sheet() {
            Some(sheet) => self.text(cx).validate().ok().as_ref() != Some(sheet.text()),
            None => false,
        }
    }

    /// Reloads when this product's run moved; a run that ended shows what
    /// it added.
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
            .product_sheet_job(product)
            .map(|(job, _)| seen(&job));
        if job != self.seen_job {
            let was_busy = self.busy();
            self.load(cx);
            if was_busy && !self.busy() {
                self.refill = true;
            }
            cx.notify();
        }
    }

    fn fail(&mut self, error: &SheetError, cx: &mut Context<Self>) {
        self.field_errors = error.field_errors().to_vec();
        self.notice = error.message().map(Notice::Error);
        cx.notify();
    }

    /// Saves the text of the form. Returns whether it is saved.
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(product) = self.product else {
            return false;
        };
        let text = self.text(cx);
        match self.bardo.read(cx).save_sheet_text(product, text) {
            Ok(_) => {
                self.load(cx);
                self.fill(window, cx);
                self.notice = Some(Notice::Saved);
                cx.notify();
                true
            }
            Err(error) => {
                self.fail(&error, cx);
                false
            }
        }
    }

    /// Starts a research or a sheet from the brief; past the budget, asks
    /// first unless `consent` says the user already agreed. Edits of the
    /// form are saved first, so the run adds to them.
    fn start(
        &mut self,
        run: SheetRun,
        consent: BudgetConsent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(product) = self.product else {
            return;
        };
        self.ask = None;
        self.notice = None;
        self.fill_error = None;
        if self.dirty(cx) && !self.save(window, cx) {
            // Said where the run was asked for; what to fix is marked in
            // the form below.
            self.fill_error = Some(if self.field_errors.is_empty() {
                Text::SheetNotSaved
            } else {
                Text::SheetFixFirst
            });
            cx.notify();
            return;
        }
        self.notice = None;
        let started = match run {
            SheetRun::Research => self.bardo.read(cx).research_product(product, consent),
            SheetRun::Brief => self.bardo.read(cx).write_sheet_from_brief(product, consent),
        };
        match started {
            Ok(_) => self.load(cx),
            Err(SheetError::OverBudget(estimate)) => self.ask = Some((run, estimate)),
            Err(error) => self.fill_error = error.message(),
        }
        cx.notify();
    }

    fn by_hand(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(product) = self.product else {
            return;
        };
        self.fill_error = None;
        match self.bardo.read(cx).start_product_sheet(product) {
            Ok(_) => {
                self.notice = None;
                self.load(cx);
                self.fill(window, cx);
            }
            Err(error) => self.fill_error = error.message(),
        }
        cx.notify();
    }

    /// Applies a change to a claim and shows the sheet as it is after it.
    fn claim_changed(
        &mut self,
        changed: Result<ProductSheet, SheetError>,
        cx: &mut Context<Self>,
    ) -> bool {
        let ok = match changed {
            Ok(_) => {
                self.claim_error = None;
                true
            }
            Err(error) => {
                self.claim_error = error.message();
                false
            }
        };
        self.load(cx);
        cx.notify();
        ok
    }

    fn add_claim(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(product) = self.product else {
            return;
        };
        let text = self.new_claim.read(cx).value().to_string();
        let added = self.bardo.read(cx).add_claim(product, &text, self.new_kind);
        if self.claim_changed(added, cx) {
            self.new_claim
                .update(cx, |input, cx| input.set_value("", window, cx));
        }
    }

    fn edit_claim(&mut self, claim: &Claim, window: &mut Window, cx: &mut Context<Self>) {
        let value = claim.text().to_owned();
        let text = cx.new(|cx| {
            let mut input = InputState::new(window, cx);
            input.set_value(value, window, cx);
            input
        });
        self.editing = Some(ClaimEdit {
            claim: claim.id,
            text,
            kind: claim.kind,
        });
        self.claim_error = None;
        cx.notify();
    }

    fn save_claim(&mut self, cx: &mut Context<Self>) {
        let (Some(product), Some(edit)) = (self.product, self.editing.as_ref()) else {
            return;
        };
        let (claim, kind) = (edit.claim, edit.kind);
        let text = edit.text.read(cx).value().to_string();
        let stored = self.sheet().and_then(|sheet| sheet.claim(claim)).cloned();
        let bardo = self.bardo.read(cx);
        let mut changed = match &stored {
            // The same words: not an edit, so no time of edit.
            Some(stored) if stored.text() == text.trim() => bardo
                .product_sheet(product)
                .and_then(|view| view.sheet.ok_or(SheetError::NoSheet)),
            _ => bardo.edit_claim(product, claim, &text),
        };
        if changed.is_ok() && stored.is_some_and(|stored| stored.kind != kind) {
            changed = bardo.set_claim_kind(product, claim, kind);
        }
        if self.claim_changed(changed, cx) {
            self.editing = None;
        }
    }

    fn render_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        v_flex()
            .gap_1()
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(kit::title(tr(bardo, Text::SheetTitle)))
                    .child(guide::info(
                        bardo,
                        "sheet-info",
                        tr(bardo, Text::SheetInfo),
                        guide::refs::PRODUCTS_SHEET,
                    )),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(tr(bardo, Text::SheetHint)),
            )
            .into_any_element()
    }

    /// What the latest run is doing, or how it ended.
    fn render_job(&self, view: &SheetView, cx: &mut Context<Self>) -> Option<AnyElement> {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let (job, _) = view.job.as_ref()?;
        let line = match job.state() {
            JobState::Queued | JobState::Running => {
                // A research is done once its notes are kept: the rest of
                // the run writes the sheet.
                let researching = view.is_researching();
                return Some(
                    v_flex()
                        .gap_1()
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(Spinner::new().small())
                                .child(div().text_sm().text_color(theme.muted_foreground).child(
                                    tr(
                                        bardo,
                                        if researching {
                                            Text::SheetResearching
                                        } else {
                                            Text::SheetWriting
                                        },
                                    ),
                                )),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(tr(bardo, Text::SheetBusyNote)),
                        )
                        .into_any_element(),
                );
            }
            JobState::Done => {
                let waiting = view.sheet.as_ref().map_or(0, ProductSheet::waiting);
                let done = kit::notice(
                    Tone::Success,
                    bardo.text_with(Text::SheetJobDone, &[("n", &waiting.to_string())]),
                    cx,
                );
                if view.left_out == 0 {
                    done
                } else {
                    // The run wrote more claims than the sheet had places.
                    return Some(
                        v_flex()
                            .gap_2()
                            .child(done)
                            .child(kit::notice(
                                Tone::Warning,
                                bardo.text_with(
                                    Text::SheetJobLeftOut,
                                    &[("n", &view.left_out.to_string())],
                                ),
                                cx,
                            ))
                            .into_any_element(),
                    );
                }
            }
            JobState::Cancelled => kit::notice(Tone::Neutral, tr(bardo, Text::SheetJobStopped), cx),
            JobState::Failed => match job.failure() {
                Some(failure) if failure.kind == JobFailureKind::Declined => kit::notice(
                    Tone::Warning,
                    bardo.text_with(Text::SheetJobDeclined, &[("reason", &failure.detail)]),
                    cx,
                ),
                Some(failure) if failure.kind == JobFailureKind::AnswerLost => {
                    kit::notice(Tone::Danger, tr(bardo, Text::SheetJobLost), cx)
                }
                _ => kit::notice(Tone::Danger, tr(bardo, Text::SheetJobFailed), cx),
            },
        };
        Some(line.into_any_element())
    }

    /// The three ways to fill the sheet, each with what it costs.
    fn render_fill(&self, view: &SheetView, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let busy = view.is_busy();
        let held = busy || self.ask.is_some();
        let has_link = view.has_link;
        let researched = view
            .sheet
            .as_ref()
            .is_some_and(|sheet| sheet.researched_at.is_some());
        let hint = |text: SharedString| {
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(text)
        };
        let limits = ResearchLimits::PRODUCT;
        let ask = self.ask.as_ref().map(|(run, estimate)| {
            let run = *run;
            budget_question(
                "sheet-budget",
                bardo,
                estimate,
                cx,
                cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.start(run, BudgetConsent::Confirmed, window, cx)
                }),
                cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.ask = None;
                    cx.notify();
                }),
            )
        });
        let research = v_flex()
            .gap_1()
            .child(
                h_flex().child(
                    Button::new("research-product")
                        .small()
                        .primary()
                        .label(tr(
                            bardo,
                            if researched {
                                Text::SheetResearchAgain
                            } else {
                                Text::SheetResearch
                            },
                        ))
                        .loading(busy)
                        .disabled(held)
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.start(SheetRun::Research, BudgetConsent::Ask, window, cx)
                        })),
                ),
            )
            .child(hint(SharedString::from(bardo.text_with(
                Text::SheetResearchHint,
                &[
                    ("searches", &limits.searches.to_string()),
                    ("fetches", &limits.fetches.to_string()),
                ],
            ))))
            .when(!has_link, |column| {
                column.child(hint(tr(bardo, Text::SheetResearchNoLink)))
            })
            .children(estimate_note(
                bardo,
                &view.research,
                Text::SheetResearchCost,
                cx,
            ))
            .children(view.research_usual.map(|usual| {
                hint(SharedString::from(bardo.text_with(
                    Text::SheetResearchUsual,
                    &[("amount", &bardo.money(usual))],
                )))
            }));
        let from_brief = v_flex()
            .gap_1()
            .child(
                h_flex().child(
                    Button::new("sheet-from-brief")
                        .small()
                        .outline()
                        .label(tr(bardo, Text::SheetFromBrief))
                        .disabled(held)
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.start(SheetRun::Brief, BudgetConsent::Ask, window, cx)
                        })),
                ),
            )
            .child(hint(tr(bardo, Text::SheetFromBriefHint)))
            .children(estimate_note(
                bardo,
                &view.from_brief,
                Text::SheetFromBriefCost,
                cx,
            ));
        let by_hand = view.sheet.is_none().then(|| {
            v_flex()
                .gap_1()
                .child(
                    h_flex().child(
                        Button::new("sheet-by-hand")
                            .small()
                            .ghost()
                            .label(tr(bardo, Text::SheetByHand))
                            .disabled(held)
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.by_hand(window, cx)
                            })),
                    ),
                )
                .child(hint(tr(bardo, Text::SheetByHandHint)))
        });
        kit::card(cx)
            .p_4()
            .gap_4()
            .child(
                kit::section_heading(tr(bardo, Text::SheetFillTitle)).child(guide::info(
                    bardo,
                    "sheet-research-info",
                    tr(bardo, Text::SheetInfo),
                    guide::refs::PRODUCTS_RESEARCH,
                )),
            )
            .child(research)
            .child(from_brief)
            .children(by_hand)
            .children(ask)
            .children(
                self.fill_error
                    .map(|error| kit::notice(Tone::Danger, tr(bardo, error), cx)),
            )
            .children(self.render_job(view, cx))
            .into_any_element()
    }

    fn error_for(&self, fields: &[SheetFieldError], cx: &App) -> Option<AnyElement> {
        let bardo = self.bardo.read(cx);
        let error = self.field_errors.iter().find(|e| fields.contains(e))?;
        Some(
            div()
                .text_xs()
                .text_color(cx.theme().danger)
                .child(tr(bardo, Text::SheetFieldError(*error)))
                .into_any_element(),
        )
    }

    fn render_restricted(&self, busy: bool, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let choices: Vec<Option<RestrictedCategory>> = std::iter::once(None)
            .chain(RestrictedCategory::ALL.into_iter().map(Some))
            .collect();
        // Eight choices do not fit one line of a narrow form: each is a
        // button of its own, so the row wraps.
        let group = h_flex()
            .flex_wrap()
            .gap_1()
            .children(choices.into_iter().enumerate().map(|(ix, choice)| {
                Button::new(("sheet-restricted", ix))
                    .small()
                    .outline()
                    .label(match choice {
                        Some(category) => tr(bardo, Text::RestrictedCategoryName(category)),
                        None => tr(bardo, Text::SheetRestrictedNone),
                    })
                    .selected(choice == self.restricted)
                    .disabled(busy)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.restricted = choice;
                        this.notice = None;
                        cx.notify();
                    }))
            }));
        kit::field(
            tr(bardo, Text::SheetRestricted),
            Some(guide::info(
                bardo,
                "sheet-restricted-info",
                tr(bardo, Text::SheetRestrictedHint),
                guide::refs::PRODUCTS_SHEET,
            )),
            group.into_any_element(),
            None,
        )
        .into_any_element()
    }

    fn render_about(&self, busy: bool, cx: &mut Context<Self>) -> AnyElement {
        let restricted = self.render_restricted(busy, cx);
        let summary_error = self.error_for(&[SheetFieldError::SummaryTooLong], cx);
        let category_error = self.error_for(&[SheetFieldError::CategoryTooLong], cx);
        let looks_error = self.error_for(&[SheetFieldError::LooksTooLong], cx);
        let bardo = self.bardo.read(cx);
        kit::card(cx)
            .p_4()
            .gap_4()
            .child(kit::section_heading(tr(bardo, Text::SheetAboutTitle)))
            .child(kit::field(
                tr(bardo, Text::SheetSummary),
                None,
                Textarea::new(&self.summary)
                    .disabled(busy)
                    .into_any_element(),
                summary_error,
            ))
            .child(kit::field(
                tr(bardo, Text::SheetCategory),
                None,
                Input::new(&self.category).disabled(busy).into_any_element(),
                category_error,
            ))
            .child(restricted)
            .child(kit::field(
                tr(bardo, Text::SheetLooks),
                None,
                Textarea::new(&self.looks).disabled(busy).into_any_element(),
                looks_error,
            ))
            .into_any_element()
    }

    /// One of the lists edited row by row.
    fn render_part(&self, part: Part, busy: bool, cx: &mut Context<Self>) -> AnyElement {
        let error = self.error_for(part.errors(), cx);
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let rows = &self.rows[part.index()];
        let labels = part.fields();
        // Each field keeps its name above it: once filled, a placeholder
        // no longer says which is which.
        let listed = rows.iter().enumerate().map(|(ix, row)| {
            let mut remove = Some(
                Button::new((part.id(), ix))
                    .xsmall()
                    .ghost()
                    .label(tr(bardo, Text::SheetRemoveRow))
                    .disabled(busy)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        let rows = &mut this.rows[part.index()];
                        if ix < rows.len() {
                            rows.remove(ix);
                        }
                        this.notice = None;
                        cx.notify();
                    })),
            );
            let fields = row.iter().zip(labels).map(|(input, label)| {
                v_flex()
                    .gap_0p5()
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(tr(bardo, *label)),
                            )
                            // Beside the first field's name.
                            .children(remove.take()),
                    )
                    .child(Input::new(input).disabled(busy))
            });
            kit::well(cx)
                .p_3()
                .gap_2()
                .children(fields.collect::<Vec<_>>())
        });
        let empty = rows.is_empty().then(|| {
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(tr(bardo, Text::SheetRowsEmpty))
        });
        let full = rows.len() >= part.limit();
        kit::card(cx)
            .p_4()
            .gap_3()
            .child(kit::section_heading(tr(bardo, part.title())))
            .children(empty)
            .children(listed)
            .children(error)
            .child(
                h_flex().child(
                    Button::new(SharedString::from(format!("add-{}", part.id())))
                        .small()
                        .outline()
                        .label(tr(bardo, part.add()))
                        .disabled(busy || full)
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            let row = this.row(part, &[], window, cx);
                            this.rows[part.index()].push(row);
                            this.notice = None;
                            cx.notify();
                        })),
                ),
            )
            .into_any_element()
    }

    fn render_save(&self, busy: bool, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let notice = self.notice.map(|notice| match notice {
            Notice::Saved => kit::notice(Tone::Success, tr(bardo, Text::SheetSaved), cx),
            Notice::Error(text) => kit::notice(Tone::Danger, tr(bardo, text), cx),
        });
        h_flex()
            .flex_wrap()
            .gap_3()
            .child(
                Button::new("save-sheet")
                    .primary()
                    .label(tr(bardo, Text::SaveSheet))
                    .disabled(busy)
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.save(window, cx);
                    })),
            )
            .children(notice.map(|notice| notice.flex_1().min_w_0()))
            .into_any_element()
    }

    /// Where a claim comes from, with the page one click away.
    fn source_line(&self, claim: &Claim, ix: usize, cx: &App) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let (name, detail) = match claim.source() {
            ClaimSource::Typed => (Text::ClaimSourceTyped, None),
            ClaimSource::Brief => (Text::ClaimSourceBrief, None),
            ClaimSource::ProductPage { .. } => (Text::ClaimSourcePage, None),
            ClaimSource::Web { title, .. } => (
                Text::ClaimSourceWeb,
                (!title.is_empty()).then(|| title.clone()),
            ),
        };
        let mut parts = vec![tr(bardo, name).to_string()];
        parts.extend(detail);
        if let Some(at) = claim
            .confirmed_at()
            .filter(|_| claim.state() == ClaimState::Confirmed)
        {
            parts.push(bardo.text_with(
                Text::ClaimConfirmedAt,
                &[("when", &bardo.product_time_text(at))],
            ));
        }
        if let Some(at) = claim.edited_at() {
            parts.push(bardo.text_with(
                Text::ClaimEditedAt,
                &[("when", &bardo.product_time_text(at))],
            ));
        }
        // Only a web address is opened: the text came from a provider.
        let link = claim
            .source()
            .url()
            .filter(|url| url.starts_with("https://") || url.starts_with("http://"))
            .map(|url| {
                let url = url.to_owned();
                Button::new(("claim-source", ix))
                    .xsmall()
                    .link()
                    .label(tr(bardo, Text::ClaimOpenSource))
                    .tooltip(url.clone())
                    .on_click(move |_, _, cx| cx.open_url(&url))
            });
        h_flex()
            .flex_wrap()
            .gap_2()
            .items_center()
            .child(kit::status(
                Tone::Neutral,
                tr(bardo, Text::ClaimKindName(claim.kind)),
                cx,
            ))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(SharedString::from(parts.join(" · "))),
            )
            .children(link)
            .into_any_element()
    }

    fn render_claim(
        &self,
        claim: &Claim,
        ix: usize,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = claim.id;
        let source = self.source_line(claim, ix, cx);
        let bardo = self.bardo.read(cx);
        if let Some(edit) = self.editing.as_ref().filter(|edit| edit.claim == id) {
            let kinds = ButtonGroup::new(("claim-kind", ix))
                .xsmall()
                .outline()
                .children(ClaimKind::ALL.map(|kind| {
                    Button::new(("claim-kind-choice", kind as usize))
                        .label(tr(bardo, Text::ClaimKindName(kind)))
                        .selected(kind == edit.kind)
                }))
                .on_click(cx.listener(|this, clicked: &Vec<usize>, _, cx| {
                    if let (Some(kind), Some(edit)) = (
                        clicked.first().and_then(|&i| ClaimKind::ALL.get(i)),
                        this.editing.as_mut(),
                    ) {
                        edit.kind = *kind;
                        cx.notify();
                    }
                }));
            return kit::well(cx)
                .p_3()
                .gap_2()
                .child(Input::new(&edit.text))
                .child(h_flex().flex_wrap().child(kinds))
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new(("save-claim", ix))
                                .small()
                                .primary()
                                .label(tr(bardo, Text::SaveClaim))
                                .on_click(
                                    cx.listener(|this, _: &ClickEvent, _, cx| this.save_claim(cx)),
                                ),
                        )
                        .child(
                            Button::new(("cancel-claim", ix))
                                .small()
                                .ghost()
                                .label(tr(bardo, Text::CancelClaimEdit))
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.editing = None;
                                    this.claim_error = None;
                                    cx.notify();
                                })),
                        ),
                )
                .into_any_element();
        }
        let act = |label: Text,
                   name: &'static str,
                   change: fn(&Bardo, ProductId, ClaimId) -> Result<ProductSheet, SheetError>| {
            Button::new((name, ix))
                .small()
                .label(tr(bardo, label))
                .disabled(busy)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    let Some(product) = this.product else {
                        return;
                    };
                    let changed = change(this.bardo.read(cx), product, id);
                    this.claim_changed(changed, cx);
                }))
        };
        let edit = {
            let claim = claim.clone();
            Button::new(("edit-claim", ix))
                .small()
                .ghost()
                .label(tr(bardo, Text::EditClaim))
                .disabled(busy)
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.edit_claim(&claim, window, cx)
                }))
        };
        let actions = match claim.state() {
            ClaimState::Found => h_flex()
                .gap_2()
                .child(act(Text::ConfirmClaim, "confirm-claim", Bardo::confirm_claim).primary())
                .child(act(Text::RejectClaim, "reject-claim", Bardo::reject_claim).outline())
                .child(edit),
            ClaimState::Confirmed => h_flex()
                .gap_2()
                .child(act(Text::RejectClaim, "reject-claim", Bardo::reject_claim).outline())
                .child(edit),
            ClaimState::Rejected => h_flex().gap_2().child(
                act(
                    Text::BringClaimBack,
                    "bring-claim-back",
                    Bardo::bring_claim_back,
                )
                .outline(),
            ),
        };
        let sensitive = Checkbox::new(("claim-sensitive", ix))
            .small()
            .checked(claim.sensitive())
            .disabled(busy)
            .label(tr(bardo, Text::ClaimSensitive))
            .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                let Some(product) = this.product else {
                    return;
                };
                let changed = this
                    .bardo
                    .read(cx)
                    .set_claim_sensitive(product, id, *checked);
                this.claim_changed(changed, cx);
            }));
        kit::well(cx)
            .p_3()
            .gap_2()
            .child(
                div()
                    .text_sm()
                    .child(SharedString::from(claim.text().to_owned())),
            )
            .child(source)
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_3()
                    .items_center()
                    .justify_between()
                    .child(actions)
                    .child(sensitive),
            )
            .into_any_element()
    }

    fn render_claims(
        &self,
        sheet: &ProductSheet,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let groups = [
            (ClaimState::Found, Text::SheetClaimsWaiting),
            (ClaimState::Confirmed, Text::SheetClaimsConfirmed),
            (ClaimState::Rejected, Text::SheetClaimsRejected),
        ];
        let mut listed: Vec<AnyElement> = Vec::new();
        for (state, title) in groups {
            let claims: Vec<(usize, &Claim)> = sheet
                .claims()
                .iter()
                .enumerate()
                .filter(|(_, claim)| claim.state() == state)
                .collect();
            if claims.is_empty() {
                continue;
            }
            let heading = SharedString::from(
                self.bardo
                    .read(cx)
                    .text_with(title, &[("n", &claims.len().to_string())]),
            );
            listed.push(
                div()
                    .text_sm()
                    .font_medium()
                    .child(heading)
                    .into_any_element(),
            );
            for (ix, claim) in claims {
                listed.push(self.render_claim(claim, ix, busy, cx));
            }
        }
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let empty = sheet.claims().is_empty().then(|| {
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(tr(bardo, Text::SheetClaimsNone))
        });
        let kinds = ButtonGroup::new("new-claim-kind")
            .xsmall()
            .outline()
            .disabled(busy)
            .children(ClaimKind::ALL.map(|kind| {
                Button::new(("new-claim-kind", kind as usize))
                    .label(tr(bardo, Text::ClaimKindName(kind)))
                    .selected(kind == self.new_kind)
            }))
            .on_click(cx.listener(|this, clicked: &Vec<usize>, _, cx| {
                if let Some(kind) = clicked.first().and_then(|&i| ClaimKind::ALL.get(i)) {
                    this.new_kind = *kind;
                    cx.notify();
                }
            }));
        let error = self
            .claim_error
            .map(|error| kit::notice(Tone::Danger, tr(bardo, error), cx));
        let add = v_flex()
            .gap_2()
            .child(Input::new(&self.new_claim).disabled(busy))
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .items_center()
                    .child(kinds)
                    .child(
                        Button::new("add-claim")
                            .small()
                            .outline()
                            .label(tr(bardo, Text::AddClaim))
                            .disabled(busy)
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.add_claim(window, cx)
                            })),
                    ),
            );
        kit::card(cx)
            .p_4()
            .gap_3()
            .child(
                kit::section_heading(tr(bardo, Text::SheetClaimsTitle)).child(guide::info(
                    bardo,
                    "sheet-claims-info",
                    tr(bardo, Text::ClaimSensitiveHint),
                    guide::refs::PRODUCTS_CLAIMS,
                )),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(tr(bardo, Text::SheetClaimsHint)),
            )
            .children(empty)
            .children(listed)
            .children(error)
            .child(add)
            .into_any_element()
    }

    fn render_sources(&self, sheet: &ProductSheet, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let when = match sheet.researched_at {
            Some(at) => SharedString::from(bardo.text_with(
                Text::SheetResearchedAt,
                &[("when", &bardo.product_time_text(at))],
            )),
            None => tr(bardo, Text::SheetSourcesNone),
        };
        let pages = sheet.sources().iter().enumerate().map(|(ix, page)| {
            let title = if page.title.is_empty() {
                page.url.clone()
            } else {
                page.title.clone()
            };
            let link =
                (page.url.starts_with("https://") || page.url.starts_with("http://")).then(|| {
                    let url = page.url.clone();
                    Button::new(("sheet-source", ix))
                        .xsmall()
                        .link()
                        .label(tr(bardo, Text::ClaimOpenSource))
                        .tooltip(url.clone())
                        .on_click(move |_, _, cx| cx.open_url(&url))
                });
            h_flex()
                .flex_wrap()
                .gap_2()
                .items_center()
                .child(div().min_w_0().text_sm().child(SharedString::from(title)))
                .children(link)
        });
        kit::card(cx)
            .p_4()
            .gap_2()
            .child(kit::section_heading(tr(bardo, Text::SheetSourcesTitle)))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(when),
            )
            .children(pages)
            .into_any_element()
    }
}

impl Render for SheetSection {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.refill {
            self.fill(window, cx);
        }
        let header = self.render_header(cx);
        let column = v_flex().max_w(px(720.)).gap_4().child(header);
        if self.product.is_none() {
            let bardo = self.bardo.read(cx);
            return column.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr(bardo, Text::SheetSaveFirst)),
            );
        }
        let Some(view) = self.view.clone() else {
            let bardo = self.bardo.read(cx);
            return column.child(kit::notice(
                Tone::Danger,
                tr(bardo, Text::SheetNotLoaded),
                cx,
            ));
        };
        let busy = view.is_busy();
        let scroll = Some(&self.scroll);
        let fill = self.render_fill(&view, cx);
        let column = column.child(kit::anchor_in(
            TourAnchor::Control(Control::ProductResearch),
            fill,
            scroll,
        ));
        let Some(sheet) = view.sheet.as_ref() else {
            return column;
        };
        let restricted = sheet.text().restricted.map(|category| {
            let bardo = self.bardo.read(cx);
            kit::notice(
                Tone::Warning,
                bardo.text_with(
                    Text::SheetRestrictedNotice,
                    &[(
                        "category",
                        &bardo.text(Text::RestrictedCategoryName(category)),
                    )],
                ),
                cx,
            )
        });
        let claims = self.render_claims(sheet, busy, cx);
        let sources = self.render_sources(sheet, cx);
        let about = self.render_about(busy, cx);
        let parts = Part::ALL.map(|part| self.render_part(part, busy, cx));
        let save = self.render_save(busy, cx);
        column
            .children(restricted)
            .child(kit::anchor_in(
                TourAnchor::Control(Control::ProductClaims),
                claims,
                scroll,
            ))
            .child(about)
            .children(parts)
            .child(save)
            .child(sources)
    }
}
