//! The scene chat on the Scenes stage (#368, ADR-0049): beside the picked
//! scene's image, the user writes what should change, Claude answers with
//! what it understood and the options with their prices, and applying one
//! starts the edit, whose image waits for review like every new image.

use bardo_app::bardo_domain::{
    ChatOptionKind, JobFailure, SceneChatMessageId, ScenePlanId, VideoProjectId,
};
use bardo_app::{
    AppliedState, BudgetConsent, ChatAvailability, ChatLine, Control, SceneChatView, SceneError,
    SpendEstimate, Text, TourAnchor,
};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, ClickEvent, Entity, SharedString, Window, div};

use super::ProjectsScreen;
use crate::guide;
use crate::kit::{self, Tone};
use crate::shell::tr;
use crate::spend::budget_question;

/// What the chat sends that a budget held back.
#[derive(Clone)]
pub(super) enum ChatAsk {
    Send(String),
    Apply(SceneChatMessageId, usize),
}

/// The scene chat as the projects screen holds it.
pub(super) struct SceneChat {
    /// The scene whose chat is open: its plan and position.
    open: Option<(ScenePlanId, usize)>,
    view: Option<SceneChatView>,
    /// The request being typed; made again on each open, so its
    /// placeholder is in the language of the moment.
    input: Option<Entity<TextareaState>>,
    error: Option<Text>,
    /// A request or an option held back at a budget, and what it costs.
    ask: Option<(ChatAsk, SpendEstimate)>,
}

impl SceneChat {
    pub(super) fn new() -> Self {
        Self {
            open: None,
            view: None,
            input: None,
            error: None,
            ask: None,
        }
    }

    /// Closes the chat: another video, or the plan it belonged to went.
    pub(super) fn reset(&mut self) {
        *self = Self::new();
    }

    /// Whether scene `index` of `plan` has its chat open.
    fn is_open(&self, plan: ScenePlanId, index: usize) -> bool {
        self.open == Some((plan, index))
    }
}

impl ProjectsScreen {
    /// Reads the open chat again, after the scenes did; a chat whose plan
    /// went closes.
    pub(super) fn load_scene_chat(&mut self, id: VideoProjectId, cx: &mut Context<Self>) {
        let Some((plan, index)) = self.scene_chat.open else {
            return;
        };
        let current = self
            .scenes
            .as_ref()
            .and_then(|view| view.plan.as_ref())
            .map(|plan| plan.id);
        if current != Some(plan) {
            self.scene_chat.reset();
            return;
        }
        match self.bardo.read(cx).scene_chat(id, index) {
            Ok(view) => self.scene_chat.view = Some(view),
            Err(error) => {
                self.scene_chat.view = None;
                self.scene_chat.error = Some(error.message());
            }
        }
    }

    fn open_scene_chat(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some((id, plan)) = self.project.zip(
            self.scenes
                .as_ref()
                .and_then(|view| view.plan.as_ref())
                .map(|plan| plan.id),
        ) else {
            return;
        };
        self.scene_chat.reset();
        self.scene_chat.open = Some((plan, index));
        let placeholder = self
            .bardo
            .read(cx)
            .text(Text::SceneChatPlaceholder)
            .to_string();
        self.scene_chat.input = Some(cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(2, 6)
                .placeholder(placeholder)
        }));
        self.load_scene_chat(id, cx);
        cx.notify();
    }

    fn close_scene_chat(&mut self, cx: &mut Context<Self>) {
        self.scene_chat.reset();
        cx.notify();
    }

    /// Sends the typed request, or applies an option; a budget in the way
    /// is asked in the chat.
    fn run_scene_chat(
        &mut self,
        action: ChatAsk,
        consent: BudgetConsent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((id, (_, index))) = self.project.zip(self.scene_chat.open) else {
            return;
        };
        self.scene_chat.ask = None;
        let bardo = self.bardo.read(cx);
        let result = match &action {
            ChatAsk::Send(text) => bardo.send_scene_chat(id, index, text, consent),
            ChatAsk::Apply(message, option) => {
                bardo.apply_scene_chat_option(id, *message, *option, consent)
            }
        };
        self.scene_chat.error = match result {
            Ok(_) => {
                if matches!(action, ChatAsk::Send(_))
                    && let Some(input) = &self.scene_chat.input
                {
                    input.update(cx, |input, cx| input.set_value("", window, cx));
                }
                None
            }
            Err(error) => match error.over_budget() {
                Some(estimate) => {
                    self.scene_chat.ask = Some((action, estimate.clone()));
                    None
                }
                None => Some(error.message()),
            },
        };
        self.load(window, cx);
        cx.notify();
    }

    fn send_scene_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(input) = &self.scene_chat.input else {
            return;
        };
        let text = input.read(cx).value().to_string();
        self.run_scene_chat(ChatAsk::Send(text), BudgetConsent::Ask, window, cx);
    }

    /// The chat part of scene `index`'s inspector: the button that opens
    /// it, or the open chat. `None` for a scene without an image and no
    /// chat open.
    pub(super) fn render_scene_chat(
        &self,
        index: usize,
        has_image: bool,
        editing: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let plan = self
            .scenes
            .as_ref()
            .and_then(|view| view.plan.as_ref())
            .map(|plan| plan.id)?;
        if !self.scene_chat.is_open(plan, index) {
            let bardo = self.bardo.read(cx);
            // The prompt being typed would be written over by an edit.
            return (has_image && !editing).then(|| {
                kit::anchor(
                    TourAnchor::Control(Control::SceneChat),
                    h_flex().child(
                        Button::new(("open-scene-chat", index))
                            .outline()
                            .small()
                            .label(tr(bardo, Text::SceneChatOpen))
                            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                this.open_scene_chat(index, window, cx)
                            })),
                    ),
                )
                .into_any_element()
            });
        }
        let busy = self.scenes.as_ref().is_some_and(|view| view.is_busy());
        let lines: Vec<AnyElement> = match self.scene_chat.view.as_ref() {
            Some(view) => view
                .lines
                .iter()
                .enumerate()
                .map(|(n, line)| self.render_chat_line(n, line, view, busy, cx))
                .collect(),
            None => Vec::new(),
        };
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let view = self.scene_chat.view.as_ref();
        let template = view.map_or(1, |view| view.template);
        let header = h_flex()
            .gap_2()
            .items_center()
            .child(
                div()
                    .text_sm()
                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                    .child(tr(bardo, Text::SceneChatTitle)),
            )
            .child(guide::info(
                bardo,
                ("scene-chat-info", index),
                SharedString::from(
                    bardo.text_with(Text::SceneChatHint, &[("n", &template.to_string())]),
                ),
                guide::refs::SCENES_CHAT,
            ))
            .child(div().flex_1())
            .child(
                Button::new(("close-scene-chat", index))
                    .ghost()
                    .xsmall()
                    .label(tr(bardo, Text::SceneChatClose))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.close_scene_chat(cx))),
            );

        let answering = view.is_some_and(SceneChatView::is_answering);
        let lines = if lines.is_empty() {
            vec![
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(tr(bardo, Text::SceneChatNoMessages))
                    .into_any_element(),
            ]
        } else {
            lines
        };
        let answering_note = answering.then(|| {
            h_flex()
                .gap_2()
                .items_center()
                .child(Spinner::new().xsmall())
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(tr(bardo, Text::SceneChatAnswering)),
                )
                .into_any_element()
        });
        let failure = view.and_then(SceneChatView::failure).map(|failure| {
            kit::notice(
                Tone::Danger,
                failed_line(bardo, Text::SceneChatReplyFailed, failure),
                cx,
            )
            .text_xs()
            .into_any_element()
        });
        let availability = view.map_or(ChatAvailability::Open, |view| view.availability);
        let closed_note = match availability {
            ChatAvailability::Open => None,
            ChatAvailability::NoImage => Some(Text::SceneChatNoImage),
            ChatAvailability::Composed => Some(Text::SceneChatComposedNote),
            ChatAvailability::OutOfDate => Some(SceneError::PlanOutOfDate.message()),
        }
        .map(|text| {
            kit::notice(Tone::Info, tr(bardo, text), cx)
                .text_xs()
                .into_any_element()
        });
        let error = self.scene_chat.error.map(|error| {
            kit::notice(Tone::Danger, tr(bardo, error), cx)
                .text_xs()
                .into_any_element()
        });
        let ask = self.scene_chat.ask.as_ref().map(|(action, estimate)| {
            let action = action.clone();
            budget_question(
                "scene-chat-budget",
                bardo,
                estimate,
                cx,
                cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.run_scene_chat(action.clone(), BudgetConsent::Confirmed, window, cx)
                }),
                cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.scene_chat.ask = None;
                    cx.notify();
                }),
            )
        });
        let open = availability == ChatAvailability::Open;
        let composer = self.scene_chat.input.as_ref().map(|input| {
            let price = view.map(|view| bardo.money(view.send_estimate.total()));
            let label = match &price {
                Some(price) => bardo.text_with(Text::SceneChatSend, &[("price", price)]),
                None => bardo.text(Text::SceneChatSend).to_string(),
            };
            v_flex()
                .gap_1()
                .child(Textarea::new(input).disabled(!open || answering))
                .child(
                    h_flex().child(
                        self.paid(Button::new(("send-scene-chat", index)), cx)
                            .primary()
                            .xsmall()
                            .label(SharedString::from(label))
                            .disabled(!open || answering)
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.send_scene_chat(window, cx)
                            })),
                    ),
                )
                .into_any_element()
        });

        Some(
            kit::anchor(
                TourAnchor::Control(Control::SceneChat),
                kit::card(cx)
                    .p_3()
                    .gap_2()
                    .child(header)
                    .child(v_flex().gap_2().children(lines))
                    .children(answering_note)
                    .children(failure)
                    .children(closed_note)
                    .children(error)
                    .children(ask)
                    .children(composer),
            )
            .into_any_element(),
        )
    }

    /// One message: the user's request, or Claude's reply with its options
    /// and where the applied one stands.
    fn render_chat_line(
        &self,
        n: usize,
        line: &ChatLine,
        view: &SceneChatView,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let message = &line.message;
        let request = message.is_request();
        let who = if request {
            Text::SceneChatYou
        } else {
            Text::SceneChatClaude
        };
        let bubble = div()
            .p_2()
            .rounded_md()
            .text_sm()
            .when(request, |bubble| bubble.bg(theme.secondary))
            .when(!request, |bubble| {
                bubble.border_1().border_color(theme.border)
            })
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(tr(bardo, who)),
            )
            .child(SharedString::from(message.text.clone()));
        let options = message.proposal().map(|proposal| {
            let price = bardo.money(view.edit_estimate.total());
            let applied = message.applied();
            v_flex()
                .gap_1()
                .children(
                    proposal
                        .options()
                        .iter()
                        .enumerate()
                        .map(|(option, choice)| {
                            let name = match choice.kind {
                                ChatOptionKind::EditImage => Text::SceneChatOptionEditImage,
                            };
                            let recommended = option == proposal.recommended();
                            let chosen = applied.as_ref().is_some_and(|a| a.option == option);
                            let id = message.id;
                            v_flex()
                                .gap_1()
                                .child(
                                    h_flex()
                                        .gap_2()
                                        .items_center()
                                        .flex_wrap()
                                        .child(
                                            self.paid(
                                                Button::new(("apply-scene-chat", n * 10 + option)),
                                                cx,
                                            )
                                            .when(recommended, |button| button.primary())
                                            .when(!recommended, |button| button.outline())
                                            .xsmall()
                                            .label(SharedString::from(format!(
                                                "{} · {}",
                                                bardo.text(name),
                                                bardo.text_with(
                                                    Text::SceneChatApply,
                                                    &[("price", &price)]
                                                )
                                            )))
                                            .disabled(!line.applies || busy)
                                            .on_click(
                                                cx.listener(
                                                    move |this, _: &ClickEvent, window, cx| {
                                                        this.run_scene_chat(
                                                            ChatAsk::Apply(id, option),
                                                            BudgetConsent::Ask,
                                                            window,
                                                            cx,
                                                        )
                                                    },
                                                ),
                                            ),
                                        )
                                        .when(recommended, |row| {
                                            row.child(kit::status(
                                                Tone::Info,
                                                tr(bardo, Text::SceneChatRecommended),
                                                cx,
                                            ))
                                        }),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(SharedString::from(choice.instruction.clone())),
                                )
                                .when(chosen, |column| {
                                    column.children(
                                        line.applied
                                            .as_ref()
                                            .map(|state| applied_line(bardo, state, cx)),
                                    )
                                })
                        }),
                )
        });
        // Options proposed for an image the scene no longer shows.
        let stale = (message.proposal().is_some()
            && !line.applies
            && line.applied.is_none()
            && view.availability == ChatAvailability::Open)
            .then(|| {
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(tr(bardo, Text::SceneChatStaleOption))
            });
        v_flex()
            .gap_1()
            .child(bubble)
            .children(options)
            .children(stale)
            .into_any_element()
    }
}

/// Where an applied option stands, as a short line.
fn applied_line(bardo: &bardo_app::Bardo, state: &AppliedState, cx: &App) -> AnyElement {
    let (tone, text): (Tone, SharedString) = match state {
        AppliedState::Working => (Tone::Info, tr(bardo, Text::SceneChatWorking)),
        AppliedState::Review => (Tone::Warning, tr(bardo, Text::SceneChatReview)),
        AppliedState::Used => (Tone::Success, tr(bardo, Text::SceneChatUsed)),
        AppliedState::Kept => (Tone::Info, tr(bardo, Text::SceneChatKept)),
        AppliedState::Failed(Some(failure)) => (
            Tone::Danger,
            failed_line(bardo, Text::SceneChatAppliedFailed, failure),
        ),
        AppliedState::Failed(None) => (Tone::Info, tr(bardo, Text::SceneChatAppliedStopped)),
        AppliedState::Ended => (Tone::Info, tr(bardo, Text::SceneChatEnded)),
    };
    kit::status(tone, text, cx).into_any_element()
}

/// `lead` followed by what kind of failure it was.
fn failed_line(bardo: &bardo_app::Bardo, lead: Text, failure: &JobFailure) -> SharedString {
    SharedString::from(format!(
        "{} {}",
        bardo.text(lead),
        bardo.text(Text::JobFailureKindName(failure.kind))
    ))
}
