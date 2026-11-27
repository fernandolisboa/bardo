//! Settings › This project (issue #343): the open project's own API key
//! per provider, which pays for the project's work in place of the app's,
//! and the monthly budget for what each own key pays.
//!
//! The rules live in `bardo_app` (`project_keys`, `save_project_key`,
//! `set_project_budget`); this file draws them and maps clicks to them.

use std::collections::HashMap;

use bardo_app::bardo_domain::{
    BYTEPLUS_DEFAULT_REGION, BytePlusField, BytePlusFieldError, ProjectId, Provider,
};
use bardo_app::{Bardo, Control, KeyState, ProjectKeyStatus, Text, TourAnchor};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{
    Disableable as _, IconName, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Entity, ScrollHandle, SharedString, Subscription, Task, Window,
    div, px,
};

use crate::appearance::look;
use crate::guide;
use crate::kit::{self, Tone};
use crate::shell::tr;

/// What the panel holds for one provider besides what `Bardo` knows.
struct Row {
    input: Entity<InputState>,
    /// Why the last save, removal or test did not happen.
    error: Option<Text>,
    /// The key test in flight; dropping it cancels the wait for its result.
    testing: Option<Task<()>>,
}

pub struct ProjectKeysPanel {
    bardo: Entity<Bardo>,
    /// The Settings screen's scroll, so the tour brings parts into view.
    scroll: ScrollHandle,
    /// The project the inputs were typed for; another one clears them.
    project: Option<ProjectId>,
    rows: HashMap<Provider, Row>,
    /// BytePlus's five fields, saved together as its one key (ADR-0025).
    byteplus: Vec<(BytePlusField, Entity<InputState>)>,
    byteplus_errors: Vec<BytePlusFieldError>,
    /// The provider whose budget is open in `budget_input`.
    editing_budget: Option<Provider>,
    budget_input: Entity<InputState>,
    budget_error: Option<Text>,
    _subscriptions: Vec<Subscription>,
}

impl ProjectKeysPanel {
    pub fn new(
        bardo: Entity<Bardo>,
        scroll: ScrollHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut subscriptions = vec![cx.observe(&bardo, |this, _, cx| this.follow_project(cx))];
        let rows = Provider::with_keys()
            .map(|provider| {
                // Masked: a key never shows on screen, even while typed.
                let input = cx.new(|cx| InputState::new(window, cx).masked(true));
                subscriptions.push(cx.subscribe(&input, move |this, _, event, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.row_mut(provider).error = None;
                        cx.notify();
                    }
                }));
                let row = Row {
                    input,
                    error: None,
                    testing: None,
                };
                (provider, row)
            })
            .collect();
        let byteplus = BytePlusField::ALL
            .into_iter()
            .map(|field| {
                let input = cx.new(|cx| {
                    let mut input = InputState::new(window, cx).masked(field.is_secret());
                    if field == BytePlusField::Region {
                        input.set_value(BYTEPLUS_DEFAULT_REGION, window, cx);
                    }
                    input
                });
                subscriptions.push(cx.subscribe(&input, move |this, _, event, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.byteplus_errors.retain(|error| error.field() != field);
                        this.row_mut(Provider::BytePlus).error = None;
                        cx.notify();
                    }
                }));
                (field, input)
            })
            .collect();
        let placeholder = tr(bardo.read(cx), Text::BudgetPlaceholder);
        let budget_input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        subscriptions.push(
            cx.subscribe(&budget_input, |this, _, event, cx| match event {
                InputEvent::Change => {
                    this.budget_error = None;
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.save_budget(cx),
                _ => {}
            }),
        );
        let project = bardo.read(cx).open_project().ok().flatten().map(|p| p.id);
        Self {
            bardo,
            scroll,
            project,
            rows,
            byteplus,
            byteplus_errors: Vec::new(),
            editing_budget: None,
            budget_input,
            budget_error: None,
            _subscriptions: subscriptions,
        }
    }

    fn row_mut(&mut self, provider: Provider) -> &mut Row {
        self.rows.get_mut(&provider).expect("a row per provider")
    }

    /// Another project opened: what was typed, failed or under test was
    /// for the one before.
    fn follow_project(&mut self, cx: &mut Context<Self>) {
        let open = self
            .bardo
            .read(cx)
            .open_project()
            .ok()
            .flatten()
            .map(|p| p.id);
        if open != self.project {
            self.project = open;
            for row in self.rows.values_mut() {
                row.error = None;
                row.testing = None;
            }
            self.byteplus_errors.clear();
            self.editing_budget = None;
            self.budget_error = None;
        }
        cx.notify();
    }

    fn save(
        &mut self,
        project: ProjectId,
        provider: Provider,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = self.rows[&provider].input.clone();
        let typed = input.read(cx).value();
        let result = self.bardo.update(cx, |bardo, cx| {
            let result = bardo.save_project_key(project, provider, &typed);
            cx.notify();
            result
        });
        let row = self.row_mut(provider);
        match result {
            Ok(()) => {
                row.error = None;
                row.testing = None;
                input.update(cx, |input, cx| input.set_value("", window, cx));
            }
            Err(error) => row.error = Some(error.message()),
        }
        cx.notify();
    }

    fn save_byteplus(&mut self, project: ProjectId, window: &mut Window, cx: &mut Context<Self>) {
        let typed: Vec<String> = self
            .byteplus
            .iter()
            .map(|(_, input)| input.read(cx).value().to_string())
            .collect();
        let fields = [
            typed[0].as_str(),
            typed[1].as_str(),
            typed[2].as_str(),
            typed[3].as_str(),
            typed[4].as_str(),
        ];
        let result = self.bardo.update(cx, |bardo, cx| {
            let result = bardo.save_project_byteplus_key(project, fields);
            cx.notify();
            result
        });
        match result {
            Ok(()) => {
                self.byteplus_errors.clear();
                for (field, input) in &self.byteplus {
                    if field.is_secret() {
                        input.update(cx, |input, cx| input.set_value("", window, cx));
                    }
                }
                let row = self.row_mut(Provider::BytePlus);
                row.error = None;
                row.testing = None;
            }
            Err(error) => {
                if let bardo_app::ProviderKeyError::BytePlusFields(fields) = &error {
                    self.byteplus_errors = fields.clone();
                }
                self.row_mut(Provider::BytePlus).error = Some(error.message());
            }
        }
        cx.notify();
    }

    fn remove(&mut self, project: ProjectId, provider: Provider, cx: &mut Context<Self>) {
        let result = self.bardo.update(cx, |bardo, cx| {
            let result = bardo.remove_project_key(project, provider);
            cx.notify();
            result
        });
        if self.editing_budget == Some(provider) {
            self.editing_budget = None;
        }
        let row = self.row_mut(provider);
        row.testing = None;
        row.error = result.err().map(|error| error.message());
        cx.notify();
    }

    /// Tests the project's key on a background thread; the panel stays
    /// responsive and shows the result when it arrives.
    fn test(&mut self, project: ProjectId, provider: Provider, cx: &mut Context<Self>) {
        let test = match self.bardo.read(cx).project_key_test(project, provider) {
            Ok(test) => test,
            Err(error) => {
                self.row_mut(provider).error = Some(error.message());
                cx.notify();
                return;
            }
        };
        let bardo = self.bardo.clone();
        let task = cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { test.run() })
                .await;
            bardo.update(cx, |bardo, cx| {
                bardo.record_key_test(result);
                cx.notify();
            });
            let _ = this.update(cx, |this, cx| {
                this.row_mut(provider).testing = None;
                cx.notify();
            });
        });
        let row = self.row_mut(provider);
        row.error = None;
        row.testing = Some(task);
        cx.notify();
    }

    fn edit_budget(
        &mut self,
        provider: Provider,
        budget: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.budget_input.update(cx, |input, cx| {
            input.set_value(budget.unwrap_or_default(), window, cx);
            input.focus(window, cx);
        });
        self.editing_budget = Some(provider);
        self.budget_error = None;
        cx.notify();
    }

    fn save_budget(&mut self, cx: &mut Context<Self>) {
        let (Some(project), Some(provider)) = (self.project, self.editing_budget) else {
            return;
        };
        let typed = self.budget_input.read(cx).value();
        let result = self.bardo.update(cx, |bardo, cx| {
            let result = bardo.set_project_budget(project, provider, &typed);
            cx.notify();
            result
        });
        match result {
            Ok(()) => {
                self.editing_budget = None;
                self.budget_error = None;
            }
            Err(error) => self.budget_error = Some(error.message()),
        }
        cx.notify();
    }

    fn remove_budget(&mut self, project: ProjectId, provider: Provider, cx: &mut Context<Self>) {
        let result = self.bardo.update(cx, |bardo, cx| {
            let result = bardo.remove_project_budget(project, provider);
            cx.notify();
            result
        });
        self.editing_budget = None;
        self.budget_error = result.err().map(|error| error.message());
        cx.notify();
    }

    /// BytePlus's five fields, each with what is wrong with it.
    fn render_byteplus_fields(&self, cx: &App) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let tokens = look(cx).tokens;
        let field = |field: BytePlusField| {
            let input = &self
                .byteplus
                .iter()
                .find(|(of, _)| *of == field)
                .expect("every field has an input")
                .1;
            let error = self
                .byteplus_errors
                .iter()
                .find(|error| error.field() == field)
                .map(|error| {
                    div()
                        .text_xs()
                        .text_color(tokens.danger)
                        .child(tr(bardo, Text::BytePlusFieldError(*error)))
                });
            v_flex()
                .flex_1()
                .min_w(px(220.))
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .font_medium()
                        .child(tr(bardo, Text::BytePlusFieldLabel(field))),
                )
                .child(Input::new(input))
                .children(error)
        };
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .flex_wrap()
                    .items_start()
                    .gap_3()
                    .children(BytePlusField::ALL.map(field)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(tokens.text2)
                    .child(tr(bardo, Text::BytePlusCardNote)),
            )
            .into_any_element()
    }

    /// The budget line of a provider the project has its own key for.
    fn render_budget(
        &self,
        project: ProjectId,
        provider: Provider,
        budget: Option<bardo_app::bardo_domain::Money>,
        anchored: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let tokens = look(cx).tokens;
        let editing = self.editing_budget == Some(provider);
        let id = provider as usize;
        let body: AnyElement = if editing {
            v_flex()
                .gap_1()
                .child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .child(
                            div()
                                .w(px(220.))
                                .child(Input::new(&self.budget_input).small()),
                        )
                        .child(
                            Button::new(("project-save-budget", id))
                                .primary()
                                .small()
                                .label(tr(bardo, Text::SaveBudget))
                                .on_click(
                                    cx.listener(|this, _: &ClickEvent, _, cx| this.save_budget(cx)),
                                ),
                        )
                        .child(
                            Button::new(("project-cancel-budget", id))
                                .ghost()
                                .small()
                                .label(tr(bardo, Text::CancelBudget))
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.editing_budget = None;
                                    this.budget_error = None;
                                    cx.notify();
                                })),
                        ),
                )
                .children(
                    self.budget_error
                        .map(|error| kit::notice(Tone::Danger, tr(bardo, error), cx).text_xs()),
                )
                .into_any_element()
        } else {
            let typed = budget.map(|budget| budget.to_string());
            let amount: SharedString = match budget {
                Some(budget) => bardo
                    .text_with(
                        Text::ProjectBudgetAmount,
                        &[("budget", &bardo.money(budget))],
                    )
                    .into(),
                None => tr(bardo, Text::ProjectBudgetNone),
            };
            h_flex()
                .gap_2()
                .flex_wrap()
                .items_center()
                .child(div().text_sm().text_color(tokens.text2).child(amount))
                .child(
                    Button::new(("project-edit-budget", id))
                        .ghost()
                        .xsmall()
                        .label(tr(
                            bardo,
                            if budget.is_some() {
                                Text::ChangeBudget
                            } else {
                                Text::SetBudget
                            },
                        ))
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.edit_budget(provider, typed.clone(), window, cx)
                        })),
                )
                .when(budget.is_some(), |line| {
                    line.child(
                        Button::new(("project-remove-budget", id))
                            .ghost()
                            .xsmall()
                            .label(tr(bardo, Text::RemoveBudget))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.remove_budget(project, provider, cx)
                            })),
                    )
                })
                .into_any_element()
        };
        let line = v_flex()
            .gap_1()
            .pt_2()
            .border_t_1()
            .border_color(tokens.border)
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        div()
                            .text_sm()
                            .font_medium()
                            .child(tr(bardo, Text::ProjectBudgetLabel)),
                    )
                    .child(guide::info(
                        bardo,
                        ("project-budget-info", id),
                        tr(bardo, Text::ProjectBudgetHint),
                        guide::refs::PROJECT_BUDGETS,
                    )),
            )
            .child(body);
        if anchored {
            kit::anchor_in(
                TourAnchor::Control(Control::ProjectKeyBudget),
                line,
                Some(&self.scroll),
            )
            .into_any_element()
        } else {
            line.into_any_element()
        }
    }

    fn render_card(
        &self,
        project: ProjectId,
        status: ProjectKeyStatus,
        first: bool,
        budget_anchor: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let provider = status.key.provider;
        let id = provider as usize;
        let byteplus = (provider == Provider::BytePlus).then(|| self.render_byteplus_fields(cx));
        let own = status.key.state != KeyState::NotSet;
        let budget = (own && provider.is_paid())
            .then(|| self.render_budget(project, provider, status.budget, budget_anchor, cx));
        let bardo = self.bardo.read(cx);
        let tokens = look(cx).tokens;
        let row = &self.rows[&provider];
        let testing = row.testing.is_some();
        let saved = matches!(status.key.state, KeyState::Saved { .. });
        let state = match &status.key.state {
            KeyState::NotSet => kit::status_with(
                Tone::Neutral,
                IconName::Minus,
                tr(bardo, Text::ProjectKeyUsesApp),
                cx,
            ),
            KeyState::Saved { hint } => kit::status(
                Tone::Success,
                bardo.text_with(Text::KeySaved, &[("hint", hint)]),
                cx,
            ),
            KeyState::Unreadable => kit::status(Tone::Danger, tr(bardo, Text::KeyUnreadable), cx),
        };
        let check = status.key.last_check.filter(|_| !testing).map(|check| {
            h_flex()
                .gap_2()
                .flex_wrap()
                .child(kit::status(
                    crate::settings::outcome_tone(check.outcome),
                    tr(bardo, Text::KeyCheckOutcome(check.outcome)),
                    cx,
                ))
                .children(check.detail.map(|detail| {
                    div()
                        .text_xs()
                        .text_color(tokens.text2)
                        .child(SharedString::from(
                            bardo.text_with(Text::KeyCheckDetail, &[("detail", &detail)]),
                        ))
                }))
        });
        let error = row
            .error
            .map(|error| kit::notice(Tone::Danger, tr(bardo, error), cx));
        // YouTube's key waits for the privacy policy (ADR-0012).
        let consent = (provider == Provider::YouTubeData && !bardo.privacy_accepted())
            .then(|| crate::privacy::consent(&self.bardo, "project-key", cx));
        let save = Button::new(("project-save-key", id))
            .label(tr(
                bardo,
                if saved {
                    Text::ReplaceKey
                } else {
                    Text::SaveKey
                },
            ))
            .disabled(consent.is_some())
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                if provider == Provider::BytePlus {
                    this.save_byteplus(project, window, cx)
                } else {
                    this.save(project, provider, window, cx)
                }
            }));
        let save = if saved {
            save.outline()
        } else {
            save.primary()
        };

        let card = kit::card(cx)
            .p_4()
            .gap_2()
            .child(
                h_flex()
                    .justify_between()
                    .items_start()
                    .gap_3()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(
                                div()
                                    .font_semibold()
                                    .child(tr(bardo, Text::ProviderName(provider))),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(tokens.text2)
                                    .child(tr(bardo, Text::ProviderPurpose(provider))),
                            ),
                    )
                    .child(div().flex_none().child(state)),
            )
            .children(consent)
            .children(byteplus)
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .when(provider != Provider::BytePlus, |actions| {
                        actions.child(div().flex_1().min_w(px(200.)).child(Input::new(&row.input)))
                    })
                    .child(save)
                    .when(saved, |actions| {
                        actions.child(
                            Button::new(("project-test-key", id))
                                .outline()
                                .label(tr(
                                    bardo,
                                    if testing {
                                        Text::TestingKey
                                    } else {
                                        Text::TestKey
                                    },
                                ))
                                .loading(testing)
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    if this.rows[&provider].testing.is_none() {
                                        this.test(project, provider, cx)
                                    }
                                })),
                        )
                    })
                    .when(own, |actions| {
                        actions.child(
                            Button::new(("project-remove-key", id))
                                .ghost()
                                .label(tr(bardo, Text::RemoveKey))
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.remove(project, provider, cx)
                                })),
                        )
                    }),
            )
            .children(error)
            .children(check)
            .children(budget);
        if first {
            kit::anchor_in(
                TourAnchor::Control(Control::ProjectKeyCard),
                card,
                Some(&self.scroll),
            )
            .into_any_element()
        } else {
            card.into_any_element()
        }
    }
}

impl Render for ProjectKeysPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let open = self.bardo.read(cx).open_project().ok().flatten();
        let Some(project) = open else {
            let bardo = self.bardo.read(cx);
            return v_flex()
                .gap_3()
                .child(kit::notice(
                    Tone::Neutral,
                    tr(bardo, Text::ProjectKeysNoProjectOpen),
                    cx,
                ))
                .into_any_element();
        };
        let id = project.id;
        let statuses = self.bardo.read(cx).project_keys(id);
        // The tour lights the budget of the first provider that has one.
        let budget_at = statuses.iter().position(|status| {
            status.key.state != KeyState::NotSet && status.key.provider.is_paid()
        });
        let cards: Vec<_> = statuses
            .into_iter()
            .enumerate()
            .map(|(index, status)| {
                self.render_card(id, status, index == 0, Some(index) == budget_at, cx)
            })
            .collect();
        let bardo = self.bardo.read(cx);
        let title = bardo.text_with(Text::ProjectKeysTitle, &[("name", project.name())]);
        v_flex()
            .gap_3()
            .child(kit::anchor_in(
                TourAnchor::Control(Control::ProjectKeysKept),
                v_flex()
                    .gap_1()
                    .child(kit::section_heading(title).child(guide::labeled_info(
                        bardo,
                        "project-keys-info",
                        Some(tr(bardo, Text::ProjectKeysInfo)),
                        tr(bardo, Text::ProjectKeysHint),
                        guide::refs::PROJECT_KEYS,
                    )))
                    .child(
                        div()
                            .text_sm()
                            .text_color(look(cx).tokens.text2)
                            .child(tr(bardo, Text::ProjectKeysHint)),
                    ),
                Some(&self.scroll),
            ))
            .children(cards)
            .into_any_element()
    }
}
