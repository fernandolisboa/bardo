//! AI cut suggestions in the editor (`docs/design/editor.md`, "AI cut
//! suggestions (#30)"): the toolbar toggle and its count, a pin on the
//! ruler for each suggestion with a dashed guide down the tracks, the
//! focused pin's popover, and the list that takes the inspector's place
//! while suggestions show. Scoring, accepting and rejecting go through
//! `Bardo`; accepting cuts the picture like a manual split, so undo takes
//! it back.
//!
//! A, R and Tab accept, reject and move to the next suggestion while the
//! suggestions show. Accepting or rejecting the focused one moves on to the
//! next.

use std::time::Duration;

use bardo_app::bardo_domain::{CutReasons, JobState, STRONG_CUT, Score, timecode};
use bardo_app::{
    Bardo, BudgetConsent, CutSuggestionError, SpendEstimate, SuggestionState, SuggestionView,
    SuggestionsView, Text,
};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::{ActiveTheme as _, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, KeyBinding, MouseButton, PathBuilder, SharedString, actions,
    canvas, div, fill, point, px, size,
};

use super::timeline::PIN_ROW;
use super::tokens::*;
use super::{EditorScreen, color, icon, label, tool_button};
use crate::appearance::EditorColor;
use crate::shell::tr;
use crate::spend::{budget_question, estimate_line, near_line};

actions!(cut_suggestions, [NextCut]);

/// The key context the editor takes while suggestions show, so Tab moves
/// between them instead of between focusable elements.
pub(super) const CONTEXT: &str = "CutSuggestions";

pub(super) fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("tab", NextCut, Some(CONTEXT))]);
}

/// What one click of the floor's − or + changes: 0.10.
const FLOOR_STEP: u8 = 10;
const POPOVER_WIDTH: f32 = 320.;
/// Pins closer than this share the ruler without their scores.
const SCORE_ROOM: f32 = 44.;

/// The suggestions' view state: none of it is saved.
#[derive(Default)]
pub(super) struct CutsState {
    /// Whether the pins and the list show.
    pub(super) on: bool,
    /// The focused suggestion, by index.
    focused: Option<usize>,
    /// Whether pending suggestions under the floor show too.
    show_all: bool,
    /// Asking would reach a budget: what it would cost.
    ask: Option<SpendEstimate>,
}

impl CutsState {
    /// Whether `item` shows on the timeline: rejected ones leave it, and
    /// pending ones under the floor wait for "Show".
    fn pinned(&self, item: &SuggestionView, floor: Score) -> bool {
        match item.state {
            SuggestionState::Pending => self.show_all || item.reaches(floor),
            SuggestionState::Accepted => true,
            SuggestionState::Rejected => false,
        }
    }

    /// Whether `item` shows in the list: everything the floor lets through.
    fn listed(&self, item: &SuggestionView, floor: Score) -> bool {
        item.state == SuggestionState::Accepted || self.show_all || item.reaches(floor)
    }

    /// The pending pins: what the toggle counts and Tab walks.
    pub(super) fn pending<'a>(
        &'a self,
        view: &'a SuggestionsView,
    ) -> impl Iterator<Item = &'a SuggestionView> {
        view.items.iter().filter(move |item| {
            item.state == SuggestionState::Pending && self.pinned(item, view.floor)
        })
    }
}

/// A suggestion's reasons, joined: "Sentence end + 420 ms pause".
fn reasons_text(bardo: &Bardo, reasons: CutReasons) -> String {
    let mut parts = Vec::new();
    if reasons.sentence_end {
        parts.push(bardo.text(Text::CutsReasonSentence).into_owned());
    }
    if let Some(pause) = reasons.pause {
        parts.push(bardo.text_with(
            Text::CutsReasonPause,
            &[("ms", &pause.as_millis().to_string())],
        ));
    }
    if reasons.scene_change {
        parts.push(bardo.text(Text::CutsReasonScene).into_owned());
    }
    if reasons.topic_shift {
        parts.push(bardo.text(Text::CutsReasonTopic).into_owned());
    }
    parts.join(" + ")
}

/// A diamond, the shape of a pin.
fn diamond(ink: EditorColor) -> AnyElement {
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _| {
            let center = bounds.center();
            let r = px(5.);
            let mut path = PathBuilder::fill();
            path.move_to(point(center.x, center.y - r));
            path.line_to(point(center.x + r, center.y));
            path.line_to(point(center.x, center.y + r));
            path.line_to(point(center.x - r, center.y));
            path.close();
            if let Ok(path) = path.build() {
                window.paint_path(path, color(ink));
            }
        },
    )
    .size(px(12.))
    .flex_none()
    .into_any_element()
}

/// A button filled with the accent: the popover's Accept.
fn primary_button(id: impl Into<gpui_kit::ElementId>) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(id)
        .h(px(28.))
        .px_2()
        .flex()
        .items_center()
        .rounded(px(4.))
        .text_size(px(12.))
        .bg(color(ACCENT))
        .text_color(color(APP))
        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
        .cursor_pointer()
        .hover(|style| style.opacity(0.9))
}

impl EditorScreen {
    fn suggestions(&self) -> Option<&SuggestionsView> {
        self.editor
            .as_ref()
            .map(|editor| &editor.view().suggestions)
    }

    /// Shows or hides the pins and the list; nothing is discarded.
    pub(super) fn toggle_cuts(&mut self, cx: &mut Context<Self>) {
        self.cuts.on = !self.cuts.on;
        if !self.cuts.on {
            self.cuts.focused = None;
            self.cuts.ask = None;
        }
        cx.notify();
    }

    /// Asks the decision engine to score the cut's open points; past a
    /// budget it asks first.
    fn suggest_cuts(&mut self, consent: BudgetConsent, cx: &mut Context<Self>) {
        let Self { bardo, editor, .. } = self;
        let Some(editor) = editor.as_mut() else {
            return;
        };
        let result = bardo.read(cx).suggest_cuts(editor, consent);
        self.cuts.ask = None;
        self.cuts.focused = None;
        self.error = match result {
            Ok(_) => None,
            Err(CutSuggestionError::OverBudget(estimate)) => {
                self.cuts.ask = Some(estimate);
                None
            }
            Err(error) => Some(error.message()),
        };
        cx.notify();
    }

    /// Runs `change` on a suggestion and, when it was the focused one,
    /// moves on to the next.
    fn change_cut(
        &mut self,
        index: usize,
        change: impl FnOnce(&Bardo, &mut bardo_app::Editor) -> Result<(), CutSuggestionError>,
        cx: &mut Context<Self>,
    ) {
        let next = (self.cuts.focused == Some(index))
            .then(|| self.next_pending(Some(index)))
            .flatten()
            .filter(|next| *next != index);
        let Self { bardo, editor, .. } = self;
        let Some(editor) = editor.as_mut() else {
            return;
        };
        let result = change(bardo.read(cx), editor);
        self.error = result.as_ref().err().map(CutSuggestionError::message);
        if result.is_ok() && self.cuts.focused == Some(index) {
            self.cuts.focused = None;
            if let Some(next) = next {
                self.focus_cut(next, cx);
            }
        }
        cx.notify();
    }

    fn accept_cut(&mut self, index: usize, cx: &mut Context<Self>) {
        self.change_cut(
            index,
            |bardo, editor| bardo.accept_suggestion(editor, index),
            cx,
        );
    }

    fn reject_cut(&mut self, index: usize, cx: &mut Context<Self>) {
        self.change_cut(
            index,
            |bardo, editor| bardo.reject_suggestion(editor, index),
            cx,
        );
    }

    fn restore_cut(&mut self, index: usize, cx: &mut Context<Self>) {
        self.change_cut(
            index,
            |bardo, editor| bardo.restore_suggestion(editor, index),
            cx,
        );
    }

    fn accept_strong_cuts(&mut self, cx: &mut Context<Self>) {
        let Self { bardo, editor, .. } = self;
        let Some(editor) = editor.as_mut() else {
            return;
        };
        let result = bardo.read(cx).accept_strong_suggestions(editor);
        self.error = result.err().map(|error| error.message());
        self.cuts.focused = None;
        cx.notify();
    }

    /// Focuses a suggestion and puts the playhead on it.
    fn focus_cut(&mut self, index: usize, cx: &mut Context<Self>) {
        let at = self
            .suggestions()
            .and_then(|view| view.items.iter().find(|item| item.index == index))
            .map(|item| item.at);
        if let Some(at) = at {
            self.cuts.focused = Some(index);
            self.seek(at, cx);
        }
    }

    /// The pending pin after suggestion `from` (or after the playhead),
    /// back to the first past the last.
    fn next_pending(&self, from: Option<usize>) -> Option<usize> {
        let view = self.suggestions()?;
        let after = from
            .and_then(|index| view.items.iter().find(|item| item.index == index))
            .map(|item| item.at)
            .or_else(|| {
                let playhead = self.editor.as_ref()?.playhead();
                // The pin under the playhead comes first.
                playhead.checked_sub(Duration::from_nanos(1))
            });
        let mut pending: Vec<&SuggestionView> = self.cuts.pending(view).collect();
        pending.sort_by_key(|item| item.at);
        pending
            .iter()
            .find(|item| after.is_none_or(|after| item.at > after))
            .or(pending.first())
            .map(|item| item.index)
    }

    /// Tab: the next pending suggestion.
    pub(super) fn next_cut(&mut self, cx: &mut Context<Self>) {
        if let Some(next) = self.next_pending(self.cuts.focused) {
            self.focus_cut(next, cx);
        }
    }

    /// A and R while the suggestions show; whether the key was theirs.
    pub(super) fn cut_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        if !self.cuts.on {
            return false;
        }
        match (key, self.cuts.focused) {
            ("a", Some(index)) => self.accept_cut(index, cx),
            ("r", Some(index)) => self.reject_cut(index, cx),
            _ => return false,
        }
        true
    }

    fn set_cut_floor(&mut self, floor: Score, cx: &mut Context<Self>) {
        let result = self.bardo.update(cx, |bardo, cx| {
            let result = bardo.set_cut_suggestion_floor(floor);
            cx.notify();
            result
        });
        self.reload(cx);
        if result.is_err() {
            self.error = Some(Text::CutsFloorNotSaved);
        }
        cx.notify();
    }

    /// Stops the scoring job, or resumes it where it stopped.
    fn cut_job(&mut self, resume: bool, cx: &mut Context<Self>) {
        let Some(job) = self
            .suggestions()
            .and_then(|view| view.job.as_ref())
            .map(|job| job.id())
        else {
            return;
        };
        let bardo = self.bardo.read(cx);
        let result = if resume {
            bardo.retry_job(job)
        } else {
            bardo.cancel_job(job)
        };
        self.reload(cx);
        if let Err(error) = result {
            self.error = Some(error.message());
        }
        cx.notify();
    }

    /// The toolbar's toggle, with the number of pending suggestions.
    pub(super) fn render_cuts_toggle(&self, editing: bool, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let mono = cx.theme().mono_font_family.clone();
        let on = self.cuts.on;
        let count = self
            .suggestions()
            .map_or(0, |view| self.cuts.pending(view).count());
        let running = self.suggestions().is_some_and(SuggestionsView::is_running);
        tool_button("toggle-cuts", editing, on)
            .when(!on, |button| {
                button.border_1().border_color(color(HAIRLINE))
            })
            .child(
                div()
                    .size(px(6.))
                    .rounded_full()
                    .bg(color(if on || running { ACCENT } else { OUTLINE })),
            )
            .child(tr(bardo, Text::EditorAiCuts))
            .when(count > 0, |button| {
                button.child(
                    div()
                        .px_1()
                        .rounded(px(3.))
                        .bg(color(RAISED))
                        .border_1()
                        .border_color(color(OUTLINE))
                        .text_size(px(10.))
                        .font_family(mono)
                        .text_color(color(TEXT))
                        .child(count.to_string()),
                )
            })
            .when(editing, |button| {
                button.on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_cuts(cx)))
            })
            .into_any_element()
    }

    /// The pins on the ruler, their guides and the focused one's popover,
    /// drawn over the lanes while the suggestions show.
    pub(super) fn render_cut_marks(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(view) = self.suggestions().filter(|_| self.cuts.on) else {
            return Vec::new();
        };
        let bardo = self.bardo.read(cx);
        let mono = cx.theme().mono_font_family.clone();
        let width = self.timeline.width();
        let mut pins: Vec<&SuggestionView> = view
            .items
            .iter()
            .filter(|item| self.cuts.pinned(item, view.floor))
            .collect();
        pins.sort_by_key(|item| item.at);
        let placed: Vec<(f32, &SuggestionView)> = pins
            .iter()
            .map(|item| (self.timeline.x(item.at), *item))
            .filter(|(x, _)| *x >= -SCORE_ROOM && *x <= width + 8.)
            .collect();
        let ruler = self.ruler_height();
        let focused = self.cuts.focused;
        let guides: Vec<(f32, EditorColor)> = placed
            .iter()
            .filter(|(_, item)| item.state == SuggestionState::Pending)
            .map(|(x, item)| {
                let ink = if focused == Some(item.index) {
                    ACCENT
                } else {
                    OUTLINE
                };
                (*x, ink)
            })
            .collect();
        let guide_lines = canvas(
            |_, _, _| {},
            move |bounds, (), window, _| {
                let left = f32::from(bounds.origin.x);
                let top = f32::from(bounds.origin.y) + ruler;
                let bottom = f32::from(bounds.origin.y + bounds.size.height);
                for (x, ink) in &guides {
                    let mut y = top;
                    while y < bottom {
                        window.paint_quad(fill(
                            gpui_kit::Bounds::new(
                                point(px(left + x), px(y)),
                                size(px(1.), px(4.0f32.min(bottom - y))),
                            ),
                            color(*ink),
                        ));
                        y += 7.;
                    }
                }
            },
        )
        .absolute()
        .inset_0()
        .into_any_element();
        let mut marks = vec![guide_lines];
        for (n, (x, item)) in placed.iter().enumerate() {
            let index = item.index;
            let is_focused = focused == Some(index);
            let room = placed
                .get(n + 1)
                .map_or(f32::INFINITY, |(next, _)| next - x);
            let pin = match item.state {
                SuggestionState::Accepted => {
                    icon(IconName::Check, TEXT_3).size_3().into_any_element()
                }
                _ => diamond(if is_focused { ACCENT } else { TEXT_2 }),
            };
            let score = (item.state == SuggestionState::Pending
                && (is_focused || room >= SCORE_ROOM))
                .then(|| {
                    label(
                        bardo.score(item.score),
                        if is_focused { ACCENT } else { TEXT_2 },
                    )
                    .text_size(px(10.))
                    .font_family(mono.clone())
                });
            marks.push(
                h_flex()
                    .id(("cut-pin", index))
                    .absolute()
                    .top(px(ruler - PIN_ROW - 1.))
                    .h(px(PIN_ROW))
                    .left(px(x - 6.))
                    .gap_0p5()
                    .items_center()
                    .cursor_pointer()
                    .child(pin)
                    .children(score)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.focus_cut(index, cx);
                        }),
                    )
                    .into_any_element(),
            );
        }
        let popover = focused.and_then(|index| {
            let (x, item) = placed
                .iter()
                .find(|(_, item)| item.index == index && item.state == SuggestionState::Pending)?;
            let left = (x - 16.).min(width - POPOVER_WIDTH - 4.).max(4.);
            Some(
                v_flex()
                    .id("cut-popover")
                    .absolute()
                    .top(px(ruler + 6.))
                    .left(px(left))
                    .w(px(POPOVER_WIDTH))
                    .p_3()
                    .gap_2()
                    .bg(color(RAISED))
                    .border_1()
                    .border_color(color(ACCENT))
                    .rounded(px(6.))
                    .shadow_lg()
                    .cursor_default()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        h_flex()
                            .justify_between()
                            .gap_2()
                            .child(
                                label(tr(bardo, Text::CutsTitle), TEXT)
                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD),
                            )
                            .child(label(timecode(item.at), TEXT_2).font_family(mono.clone())),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                label(bardo.score(item.score), ACCENT)
                                    .text_size(px(13.))
                                    .font_family(mono.clone()),
                            )
                            .child(label(
                                bardo.text_with(
                                    Text::CutsConfidence,
                                    &[("n", &item.confidence.percent().to_string())],
                                ),
                                TEXT_3,
                            )),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(color(TEXT_2))
                            .child(reasons_text(bardo, item.reasons)),
                    )
                    .child(
                        h_flex()
                            .flex_wrap()
                            .gap_1()
                            .child(
                                primary_button("cut-popover-accept")
                                    .child(tr(bardo, Text::CutsAccept))
                                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                        this.accept_cut(index, cx)
                                    })),
                            )
                            .child(
                                tool_button("cut-popover-reject", true, false)
                                    .border_1()
                                    .border_color(color(OUTLINE))
                                    .child(tr(bardo, Text::CutsReject))
                                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                        this.reject_cut(index, cx)
                                    })),
                            )
                            .child(
                                tool_button("cut-popover-next", true, false)
                                    .child(tr(bardo, Text::CutsNext))
                                    .on_click(
                                        cx.listener(|this, _: &ClickEvent, _, cx| {
                                            this.next_cut(cx)
                                        }),
                                    ),
                            ),
                    )
                    .into_any_element(),
            )
        });
        marks.extend(popover);
        marks
    }

    /// The suggestions list, in the inspector's place: asking, the job,
    /// the floor, then a row per suggestion.
    pub(super) fn render_cuts_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let mono = cx.theme().mono_font_family.clone();
        let view = self.suggestions().cloned().unwrap_or_default();
        let has_cut = self
            .editor
            .as_ref()
            .is_some_and(|editor| editor.view().timeline.is_some());
        let running = view.is_running();
        let job_state = view.job.as_ref().map(|job| job.state());

        let job = view.job.as_ref().and_then(|job| match job.state() {
            JobState::Queued | JobState::Running => Some(
                v_flex()
                    .gap_1()
                    .child(label(
                        bardo.text_with(
                            Text::CutsRunning,
                            &[("percent", &job.progress().percent().to_string())],
                        ),
                        TEXT_2,
                    ))
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div().flex_1().child(
                                    Progress::new("cuts-progress")
                                        .small()
                                        .value(job.progress().permille() as f32 / 10.0),
                                ),
                            )
                            .child(
                                tool_button("cuts-stop", true, false)
                                    .border_1()
                                    .border_color(color(OUTLINE))
                                    .child(tr(bardo, Text::CutsStop))
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.cut_job(false, cx)
                                    })),
                            ),
                    )
                    .into_any_element(),
            ),
            JobState::Failed | JobState::Cancelled => {
                let line = match job.failure() {
                    Some(failure) if job.state() == JobState::Failed => bardo
                        .text(Text::JobFailureKindName(failure.kind))
                        .into_owned(),
                    _ => bardo.text(Text::CutsStopped).into_owned(),
                };
                Some(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(color(if job.state() == JobState::Failed {
                                    ERROR
                                } else {
                                    TEXT_2
                                }))
                                .child(line),
                        )
                        .when(job.can_retry(), |column| {
                            column.child(
                                tool_button("cuts-retry", true, true)
                                    .child(tr(bardo, Text::CutsRetry))
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.cut_job(true, cx)
                                    })),
                            )
                        })
                        .into_any_element(),
                )
            }
            JobState::Done => None,
        });

        let asked_before = !view.items.is_empty();
        let ask = (!running).then(|| {
            let hint = if view.open_points == 0 {
                bardo.text(Text::CutsNoPoints).into_owned()
            } else {
                bardo.text_with(
                    if asked_before {
                        Text::CutsAgainHint
                    } else {
                        Text::CutsHint
                    },
                    &[("n", &view.open_points.to_string())],
                )
            };
            let can_ask = has_cut && view.open_points > 0;
            let button = if asked_before {
                tool_button("cuts-suggest", can_ask, false)
                    .border_1()
                    .border_color(color(OUTLINE))
                    .child(tr(bardo, Text::CutsSuggestAgain))
            } else {
                primary_button("cuts-suggest")
                    .when(!can_ask, |button| button.opacity(0.5).cursor_default())
                    .child(tr(bardo, Text::CutsSuggest))
            };
            v_flex()
                .gap_1p5()
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(color(TEXT_2))
                        .child(hint),
                )
                .child(h_flex().child(button.when(can_ask, |button| {
                    button.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.suggest_cuts(BudgetConsent::Ask, cx)
                    }))
                })))
                .children(
                    view.estimate
                        .as_ref()
                        .filter(|_| can_ask)
                        .map(|estimate| self.render_cut_estimate(bardo, estimate)),
                )
        });
        let budget = self.cuts.ask.as_ref().map(|estimate| {
            budget_question(
                "cuts-budget",
                bardo,
                estimate,
                cx,
                cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.suggest_cuts(BudgetConsent::Confirmed, cx)
                }),
                cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.cuts.ask = None;
                    cx.notify();
                }),
            )
        });

        let floor = view.floor;
        let floor_button = |id: &'static str, glyph: IconName, to: Option<Score>| {
            tool_button(id, to.is_some(), false)
                .h(px(24.))
                .min_w(px(24.))
                .px_1()
                .border_1()
                .border_color(color(OUTLINE))
                .child(icon(glyph, TEXT_2).size_3())
                .when_some(to, |button, to| {
                    button.on_click(
                        cx.listener(move |this, _: &ClickEvent, _, cx| this.set_cut_floor(to, cx)),
                    )
                })
        };
        let lower = floor.value().checked_sub(FLOOR_STEP).map(Score::new);
        let higher = (floor.value() < 100).then(|| Score::new(floor.value() + FLOOR_STEP));
        let floor_row = h_flex()
            .justify_between()
            .gap_2()
            .child(label(tr(bardo, Text::CutsFloor), TEXT_3))
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(floor_button("cuts-floor-less", IconName::Minus, lower))
                    .child(
                        label(bardo.score(floor), TEXT)
                            .font_family(mono.clone())
                            .min_w(px(48.))
                            .text_center(),
                    )
                    .child(floor_button("cuts-floor-more", IconName::Plus, higher)),
            );
        let hidden = view.hidden();
        let hidden_row = (hidden > 0).then(|| {
            h_flex()
                .justify_between()
                .gap_2()
                .child(label(
                    bardo.text_with(
                        if self.cuts.show_all {
                            Text::CutsShown
                        } else {
                            Text::CutsHidden
                        },
                        &[("n", &hidden.to_string()), ("score", &bardo.score(floor))],
                    ),
                    TEXT_3,
                ))
                .child(
                    tool_button("cuts-show-hidden", true, self.cuts.show_all)
                        .child(tr(
                            bardo,
                            if self.cuts.show_all {
                                Text::CutsHideLow
                            } else {
                                Text::CutsShowHidden
                            },
                        ))
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.cuts.show_all = !this.cuts.show_all;
                            cx.notify();
                        })),
                )
        });
        let strong = view
            .items
            .iter()
            .any(|item| item.state == SuggestionState::Pending && item.reaches(STRONG_CUT));
        let accept_strong = asked_before.then(|| {
            tool_button("cuts-accept-strong", strong, false)
                .border_1()
                .border_color(color(OUTLINE))
                .child(icon(IconName::Check, if strong { TEXT_2 } else { TEXT_3 }))
                .child(bardo.text_with(
                    Text::CutsAcceptStrong,
                    &[("score", &bardo.score(STRONG_CUT))],
                ))
                .when(strong, |button| {
                    button.on_click(
                        cx.listener(|this, _: &ClickEvent, _, cx| this.accept_strong_cuts(cx)),
                    )
                })
        });

        let rows = view
            .items
            .iter()
            .filter(|item| self.cuts.listed(item, floor))
            .map(|item| self.render_cut_row(bardo, item, mono.clone(), cx))
            .collect::<Vec<_>>();
        let empty = (rows.is_empty() && !running && job_state.is_none())
            .then(|| label(tr(bardo, Text::CutsEmpty), TEXT_3));

        v_flex()
            .w(px(320.))
            .flex_none()
            .h_full()
            .bg(color(PANEL))
            .border_l_1()
            .border_color(color(HAIRLINE))
            .child(
                h_flex()
                    .h(px(36.))
                    .px_3()
                    .items_center()
                    .border_b_1()
                    .border_color(color(HAIRLINE))
                    .child(label(tr(bardo, Text::EditorAiCuts), TEXT_2)),
            )
            .child(
                v_flex()
                    .id("cuts-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_3()
                    .gap_3()
                    .children(job)
                    .children(ask)
                    .children(budget)
                    .when(asked_before, |list| {
                        list.child(
                            v_flex()
                                .gap_1p5()
                                .pt_2()
                                .border_t_1()
                                .border_color(color(HAIRLINE))
                                .child(floor_row)
                                .children(hidden_row),
                        )
                    })
                    .children(accept_strong.map(|button| h_flex().child(button)))
                    .children(empty)
                    .child(v_flex().gap_1().children(rows)),
            )
            .into_any_element()
    }

    /// What asking costs, in the editor's look, with the budgets it takes
    /// past 80%. One it reaches is asked about when the user asks.
    fn render_cut_estimate(&self, bardo: &Bardo, estimate: &SpendEstimate) -> AnyElement {
        let near = estimate.near_budget().map(|provider| {
            h_flex()
                .gap_1()
                .items_start()
                .child(icon(IconName::TriangleAlert, ACCENT).size_3())
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(color(TEXT_2))
                        .child(near_line(bardo, provider)),
                )
        });
        v_flex()
            .gap_0p5()
            .children(
                estimate_line(bardo, estimate, Text::EstimateCost).map(|line| {
                    div()
                        .text_size(px(11.))
                        .text_color(color(TEXT_3))
                        .child(line)
                }),
            )
            .children(near)
            .into_any_element()
    }

    /// A suggestion in the list: timecode, score bar, reasons, and accept
    /// and reject, a check once cut, or Undo once rejected.
    fn render_cut_row(
        &self,
        bardo: &Bardo,
        item: &SuggestionView,
        mono: SharedString,
        cx: &Context<Self>,
    ) -> AnyElement {
        let index = item.index;
        let focused = self.cuts.focused == Some(index);
        let rejected = item.state == SuggestionState::Rejected;
        let bar = div()
            .w(px(48.))
            .h(px(4.))
            .flex_none()
            .rounded(px(2.))
            .bg(color(HAIRLINE))
            .child(
                div()
                    .h_full()
                    .w(px(48. * f32::from(item.score.value()) / 100.))
                    .rounded(px(2.))
                    .bg(color(if focused { ACCENT } else { TEXT_2 })),
            );
        let actions = match item.state {
            SuggestionState::Pending => h_flex()
                .gap_1()
                .child(
                    tool_button(("cut-accept", index), true, false)
                        .child(icon(IconName::Check, TEXT_2))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            cx.stop_propagation();
                            this.accept_cut(index, cx)
                        })),
                )
                .child(
                    tool_button(("cut-reject", index), true, false)
                        .child(icon(IconName::Close, TEXT_2))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            cx.stop_propagation();
                            this.reject_cut(index, cx)
                        })),
                ),
            SuggestionState::Accepted => h_flex()
                .gap_1()
                .items_center()
                .child(icon(IconName::Check, TEXT_2))
                .child(label(tr(bardo, Text::CutsAccepted), TEXT_2)),
            SuggestionState::Rejected => h_flex().child(
                tool_button(("cut-restore", index), true, false)
                    .child(icon(IconName::Undo, TEXT_2))
                    .child(tr(bardo, Text::CutsUndo))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        this.restore_cut(index, cx)
                    })),
            ),
        };
        let text_ink = if rejected { TEXT_3 } else { TEXT };
        h_flex()
            .id(("cut-row", index))
            .gap_2()
            .px_2()
            .py_1p5()
            .items_center()
            .rounded(px(4.))
            .when(focused, |row| {
                row.bg(color(RAISED)).border_1().border_color(color(ACCENT))
            })
            .when(!focused, |row| row.hover(|style| style.bg(color(RAISED))))
            .when(item.state == SuggestionState::Pending, |row| {
                row.cursor_pointer().on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| this.focus_cut(index, cx)),
                )
            })
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                label(timecode(item.at), text_ink)
                                    .font_family(mono.clone())
                                    .when(rejected, |text| text.line_through()),
                            )
                            .child(bar)
                            .child(
                                label(bardo.score(item.score), text_ink)
                                    .font_family(mono)
                                    .when(rejected, |text| text.line_through()),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(color(if rejected { TEXT_3 } else { TEXT_2 }))
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .when(rejected, |text| text.line_through())
                            .child(reasons_text(bardo, item.reasons)),
                    ),
            )
            .child(actions)
            .into_any_element()
    }
}
