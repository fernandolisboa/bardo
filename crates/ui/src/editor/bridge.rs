//! The AI bridge over a cut (#363, ADR-0050): the inspector's section for
//! the clip after a cut, which asks a model for a short clip from the last
//! frame before the cut to the first frame after it, and a band on V1 over
//! each bridge waiting for review. Asking, previewing, using and
//! discarding go through `Bardo`; using lays it over the cut in one
//! undoable edit.

use bardo_app::bardo_domain::{BridgeId, timecode};
use bardo_app::{
    Bardo, BridgeAsk, BridgeError, BridgeOptions, BridgeState, BridgeView, BudgetConsent,
    EditorView, SpendEstimate, Text,
};
use gpui_kit::component::input::Input;
use gpui_kit::component::{Icon, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, ClickEvent, MouseButton, MouseDownEvent, SharedString, Window, div, px,
};

use super::tokens::*;
use super::{EditorScreen, color, icon, label, tool_button};
use crate::icons::Lucide;
use crate::shell::tr;
use crate::spend::budget_question;

/// The panel that asks for a bridge, while open: none of it is saved.
#[derive(Default)]
pub(super) struct BridgeForm {
    /// What the cut offers, read when the panel opened.
    options: Option<BridgeOptions>,
    /// The picked model, by index in the options.
    model: usize,
    /// The picked length, in seconds.
    seconds: u32,
    /// Asking would reach a budget: the ask and what it would cost.
    ask: Option<(BridgeAsk, Box<SpendEstimate>)>,
}

impl BridgeForm {
    /// The cut the panel is open on.
    fn cut(&self) -> Option<usize> {
        self.options.as_ref().map(|options| options.cut)
    }
}

/// The bridge the clip at `index` shows: the one over the cut into it, or
/// one whose cut changed and whose stretch reaches into the clip.
fn bridge_for(view: &EditorView, index: usize) -> Option<&BridgeView> {
    view.bridges
        .iter()
        .find(|bridge| bridge.cut == Some(index))
        .or_else(|| {
            let clip = view.clips.get(index)?;
            let end = clip.at + clip.duration;
            view.bridges
                .iter()
                .find(|bridge| bridge.cut.is_none() && bridge.from < end && bridge.to > clip.at)
        })
}

impl EditorScreen {
    /// The inspector's AI bridge for the clip at `index`: the bridge over
    /// the cut into it, or the button and panel that ask for one. The
    /// first clip has no cut before it.
    pub(super) fn render_bridge_controls(
        &self,
        index: usize,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        let bardo = self.bardo.read(cx);
        let editor = self.editor.as_ref()?;
        let view = editor.view();
        let existing = bridge_for(view, index);
        if index == 0 && existing.is_none() {
            return None;
        }
        let hint = |text: SharedString| {
            div()
                .text_size(px(11.))
                .text_color(color(TEXT_3))
                .child(text)
        };
        let section = v_flex()
            .gap_1p5()
            .child(label(tr(bardo, Text::EditorBridge), TEXT_3));
        let body = match existing {
            Some(bridge) => self.render_bridge(bardo, bridge, editor.previewed_bridge(), cx),
            None if self.bridge.cut() == Some(index) => self.render_bridge_form(bardo, cx),
            None => {
                let open = tool_button("bridge-smooth", true, false)
                    .border_1()
                    .border_color(color(OUTLINE))
                    .child(
                        Icon::new(Lucide::Sparkles)
                            .size_4()
                            .text_color(color(ACCENT)),
                    )
                    .child(tr(bardo, Text::EditorBridgeSmooth))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.open_bridge_form(index, window, cx);
                    }));
                v_flex()
                    .gap_1p5()
                    .child(h_flex().child(open))
                    .child(hint(tr(bardo, Text::EditorBridgeHint)))
                    .into_any_element()
            }
        };
        Some(section.child(body).into_any_element())
    }

    /// A bridge asked for: being made, failed, made and waiting for
    /// review, or left behind by a cut that changed.
    fn render_bridge(
        &self,
        bardo: &Bardo,
        bridge: &BridgeView,
        previewed: Option<BridgeId>,
        cx: &Context<Self>,
    ) -> AnyElement {
        let id = bridge.id;
        let line = |text: SharedString| {
            div()
                .text_size(px(12.))
                .text_color(color(TEXT_2))
                .child(text)
        };
        let button = |name: &'static str, text: Text| {
            tool_button(name, true, false)
                .border_1()
                .border_color(color(OUTLINE))
                .child(tr(bardo, text))
        };
        let covers = line(
            bardo
                .text_with(
                    Text::EditorBridgeCovers,
                    &[
                        ("from", &timecode(bridge.from)),
                        ("to", &timecode(bridge.to)),
                    ],
                )
                .into(),
        );
        let discard = button("bridge-discard", Text::EditorBridgeDiscard).on_click(cx.listener(
            move |this, _: &ClickEvent, _, cx| {
                this.bridge_action(cx, |bardo, editor| bardo.discard_bridge(editor, id));
            },
        ));
        let mut body = v_flex().gap_1p5();
        if bridge.cut.is_none() {
            return body
                .child(line(tr(bardo, Text::EditorBridgeStale)))
                .child(h_flex().gap_1().child(discard))
                .into_any_element();
        }
        body = body.child(covers);
        match &bridge.state {
            BridgeState::Making => body
                .child(line(
                    bardo
                        .text_with(Text::EditorBridgeMaking, &[("model", &bridge.model)])
                        .into(),
                ))
                .into_any_element(),
            BridgeState::Failed(failure) => {
                let job = bridge.job;
                let retry = button("bridge-retry", Text::EditorBridgeRetry).on_click(cx.listener(
                    move |this, _: &ClickEvent, _, cx| {
                        let result = this.bardo.read(cx).retry_job(job);
                        this.error = result.err().map(|error| error.message());
                        this.reload(cx);
                    },
                ));
                body.child(label(tr(bardo, Text::EditorBridgeFailed), ERROR).whitespace_normal())
                    .children(
                        failure
                            .as_ref()
                            .map(|failure| line(tr(bardo, Text::JobFailureKindName(failure.kind)))),
                    )
                    .child(h_flex().gap_1().child(retry).child(discard))
                    .into_any_element()
            }
            BridgeState::Ready { .. } => {
                let previewing = previewed == Some(id);
                let preview = tool_button("bridge-preview", true, previewing)
                    .border_1()
                    .border_color(color(OUTLINE))
                    .child(icon(
                        if previewing {
                            IconName::Pause
                        } else {
                            IconName::Play
                        },
                        TEXT_2,
                    ))
                    .child(tr(
                        bardo,
                        if previewing {
                            Text::EditorBridgeStopPreview
                        } else {
                            Text::EditorBridgePreview
                        },
                    ))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        let Some(editor) = this.editor.as_mut() else {
                            return;
                        };
                        editor.preview_bridge((!previewing).then_some(id));
                        // It plays from a second before the bridge.
                        if !previewing && !editor.is_playing() {
                            this.toggle_play(cx);
                        }
                        cx.notify();
                    }));
                let use_it = tool_button("bridge-use", true, true)
                    .border_1()
                    .border_color(color(ACCENT))
                    .child(tr(bardo, Text::EditorBridgeUse))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.bridge_action(cx, |bardo, editor| bardo.use_bridge(editor, id));
                    }));
                body.child(line(
                    bardo
                        .text_with(
                            Text::EditorBridgeReady,
                            &[
                                ("model", &bridge.model),
                                ("seconds", &bridge.seconds.to_string()),
                            ],
                        )
                        .into(),
                ))
                .child(h_flex().gap_1().child(preview))
                .child(h_flex().gap_1().child(use_it).child(discard))
                .into_any_element()
            }
        }
    }

    /// The open panel: the models that fit, the length, a note, the price,
    /// and Make.
    fn render_bridge_form(&self, bardo: &Bardo, cx: &Context<Self>) -> AnyElement {
        let Some(options) = self.bridge.options.as_ref() else {
            return div().into_any_element();
        };
        let hint = |text: SharedString| {
            div()
                .text_size(px(11.))
                .text_color(color(TEXT_3))
                .child(text)
        };
        let cancel = tool_button("bridge-cancel", true, false)
            .border_1()
            .border_color(color(OUTLINE))
            .child(tr(bardo, Text::EditorBridgeCancel))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.bridge = BridgeForm::default();
                cx.notify();
            }));
        let refusal = options
            .refusal
            .map(|refusal| BridgeError::Refused(refusal).message())
            .or_else(|| {
                options
                    .models
                    .is_empty()
                    .then_some(Text::EditorBridgeNoModel)
            });
        if let Some(refusal) = refusal {
            return v_flex()
                .gap_1p5()
                .child(label(tr(bardo, refusal), TEXT_2).whitespace_normal())
                .child(h_flex().child(cancel))
                .into_any_element();
        }
        let picked = options.models.get(self.bridge.model);
        let models = h_flex()
            .flex_wrap()
            .gap_1()
            .children(options.models.iter().enumerate().map(|(n, model)| {
                let active = n == self.bridge.model;
                tool_button(("bridge-model", n), true, active)
                    .h(px(24.))
                    .border_1()
                    .border_color(color(OUTLINE))
                    .child(SharedString::from(model.model.name.to_string()))
                    .when(!active, |button| {
                        button.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.pick_bridge_model(n);
                            cx.notify();
                        }))
                    })
            }));
        let lengths = picked.map(|model| model.lengths.as_slice()).unwrap_or(&[]);
        let at = lengths
            .iter()
            .position(|length| length.seconds == self.bridge.seconds);
        let step = |to: Option<u32>| {
            to.map(|seconds| -> super::Click {
                std::rc::Rc::new(move |this: &mut Self, cx: &mut Context<Self>| {
                    this.bridge.seconds = seconds;
                    cx.notify();
                })
            })
        };
        let less = at
            .and_then(|at| at.checked_sub(1))
            .and_then(|at| lengths.get(at))
            .map(|length| length.seconds);
        let more = at
            .and_then(|at| lengths.get(at + 1))
            .map(|length| length.seconds);
        let price = at
            .and_then(|at| lengths.get(at))
            .map(|length| match length.price {
                Some(price) => bardo
                    .text_with(Text::EditorBridgePrice, &[("price", &bardo.price(price))])
                    .into(),
                None => tr(bardo, Text::EditorBridgeNoPrice),
            });
        let make = tool_button("bridge-make", picked.is_some(), true)
            .border_1()
            .border_color(color(ACCENT))
            .child(
                Icon::new(Lucide::Sparkles)
                    .size_4()
                    .text_color(color(ACCENT)),
            )
            .child(tr(bardo, Text::EditorBridgeMake))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.make_bridge(BudgetConsent::Ask, cx);
            }));
        let ask = self.bridge.ask.as_ref().map(|(_, estimate)| {
            budget_question(
                "bridge-budget",
                bardo,
                estimate,
                cx,
                cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.make_bridge(BudgetConsent::Confirmed, cx);
                }),
                cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.bridge.ask = None;
                    cx.notify();
                }),
            )
        });
        v_flex()
            .gap_1p5()
            .child(label(tr(bardo, Text::EditorBridgeModel), TEXT_3))
            .child(models)
            .child(self.step_row(
                "bridge-length",
                tr(bardo, Text::EditorBridgeLength),
                format!("{} s", self.bridge.seconds),
                step(less),
                step(more),
                cx,
            ))
            .child(label(tr(bardo, Text::EditorBridgeNote), TEXT_3))
            .child(
                // In the editor's dark look, not the app's.
                Input::new(&self.bridge_note)
                    .small()
                    .bg(color(APP))
                    .text_color(color(TEXT))
                    .border_color(color(OUTLINE)),
            )
            .children(price.map(hint))
            .child(hint(tr(bardo, Text::EditorBridgeHint)))
            .children(ask)
            .child(h_flex().gap_1().child(make).child(cancel))
            .into_any_element()
    }

    /// Opens the panel on the cut into the clip at `index`, with the first
    /// model and its shortest length picked and an empty note.
    fn open_bridge_form(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.editor.as_ref() else {
            return;
        };
        match self.bardo.read(cx).bridge_options(editor, index) {
            Ok(options) => {
                self.bridge = BridgeForm {
                    options: Some(options),
                    ..BridgeForm::default()
                };
                self.pick_bridge_model(0);
                self.error = None;
            }
            Err(error) => self.error = Some(error.message()),
        }
        self.bridge_note.update(cx, |input, cx| {
            input.set_placeholder(
                tr(self.bardo.read(cx), Text::EditorBridgeNotePlaceholder),
                window,
                cx,
            );
            input.set_value("", window, cx);
        });
        cx.notify();
    }

    /// Picks the model at `n`, keeping the length when it offers it and
    /// else taking its shortest.
    fn pick_bridge_model(&mut self, n: usize) {
        let Some(model) = self
            .bridge
            .options
            .as_ref()
            .and_then(|options| options.models.get(n))
        else {
            return;
        };
        self.bridge.model = n;
        if !model
            .lengths
            .iter()
            .any(|length| length.seconds == self.bridge.seconds)
        {
            self.bridge.seconds = model.lengths.first().map_or(0, |length| length.seconds);
        }
        self.bridge.ask = None;
    }

    /// Asks for the bridge the panel holds. Over a budget, the panel asks
    /// first; any other refusal shows in the banner.
    fn make_bridge(&mut self, consent: BudgetConsent, cx: &mut Context<Self>) {
        let note = self.bridge_note.read(cx).value().to_string();
        let ask = match (&self.bridge.ask, consent) {
            (Some((ask, _)), BudgetConsent::Confirmed) => ask.clone(),
            _ => {
                let Some(options) = self.bridge.options.as_ref() else {
                    return;
                };
                let Some(model) = options.models.get(self.bridge.model) else {
                    return;
                };
                BridgeAsk {
                    cut: options.cut,
                    provider: model.model.id.provider(),
                    model: model.model.id.model().to_owned(),
                    seconds: self.bridge.seconds,
                    note,
                }
            }
        };
        let Self { bardo, editor, .. } = self;
        let Some(editor) = editor.as_mut() else {
            return;
        };
        match bardo.read(cx).make_bridge(editor, ask.clone(), consent) {
            Ok(_) => {
                self.bridge = BridgeForm::default();
                self.error = None;
            }
            Err(BridgeError::OverBudget(estimate)) => {
                self.bridge.ask = Some((ask, estimate));
            }
            Err(error) => self.error = Some(error.message()),
        }
        cx.notify();
    }

    /// Runs one of the bridge's buttons, showing what refused it.
    fn bridge_action(
        &mut self,
        cx: &mut Context<Self>,
        action: impl FnOnce(&Bardo, &mut bardo_app::Editor) -> Result<(), BridgeError>,
    ) {
        let Self { bardo, editor, .. } = self;
        let Some(editor) = editor.as_mut() else {
            return;
        };
        self.error = action(bardo.read(cx), editor)
            .err()
            .map(|error| error.message());
        cx.notify();
    }

    /// A band high on V1 over each bridge waiting on the cut: dashed while
    /// being made, solid once made. Clicking one selects the clip after
    /// its cut, whose inspector shows it.
    pub(super) fn render_bridge_marks(
        &self,
        view: &EditorView,
        cx: &Context<Self>,
    ) -> Vec<AnyElement> {
        let bardo = self.bardo.read(cx);
        view.bridges
            .iter()
            .enumerate()
            .filter_map(|(n, bridge)| {
                let left = self.timeline.x(bridge.from);
                let width = (self.timeline.x(bridge.to) - left).max(8.);
                if left + width < 0. || left > self.timeline.width() {
                    return None;
                }
                let ready = bridge.is_ready() && bridge.cut.is_some();
                let ink = match (&bridge.state, bridge.cut) {
                    (_, None) | (BridgeState::Failed(_), _) => ERROR,
                    _ => ACCENT,
                };
                let name = (width > 72.).then(|| {
                    label(
                        tr(bardo, Text::EditorBridgeMarker),
                        if ready { APP } else { TEXT },
                    )
                    .text_size(px(9.))
                    .overflow_hidden()
                    .text_ellipsis()
                });
                let index = bridge.cut.or_else(|| {
                    view.clips
                        .iter()
                        .position(|clip| clip.at + clip.duration > bridge.from)
                });
                let from = bridge.from;
                Some(
                    div()
                        .id(("bridge-mark", n))
                        .absolute()
                        .top(px(1.))
                        .h(px(11.))
                        .left(px(left))
                        .w(px(width))
                        .px_1()
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap_0p5()
                        .overflow_hidden()
                        .rounded(px(3.))
                        .when(ready, |mark| mark.bg(color(ink).opacity(0.85)))
                        .when(!ready, |mark| {
                            mark.bg(color(PANEL).opacity(0.85)).border_dashed()
                        })
                        .border_1()
                        .border_color(color(ink))
                        .cursor_pointer()
                        .children(name)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                this.seek(from, cx);
                                if let Some(index) = index {
                                    this.select(
                                        Some(bardo_app::bardo_domain::ItemRef::video(index)),
                                        cx,
                                    );
                                }
                            }),
                        )
                        .into_any_element(),
                )
            })
            .collect()
    }
}
