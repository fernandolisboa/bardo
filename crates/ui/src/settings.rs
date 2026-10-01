//! Settings screen: one card per provider to save, replace, test and remove
//! its API key. Rules, storage and the test call live in `bardo_app`; this
//! file maps clicks to use cases and results to text.

use std::collections::HashMap;

use bardo_app::bardo_domain::{KeyCheckOutcome, Provider};
use bardo_app::{Bardo, KeyState, ProviderKeyStatus, Text};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{ClickEvent, Entity, Hsla, SharedString, Subscription, Task, Window, div, px};

use crate::shell::tr;

/// What the screen holds for one provider besides what `Bardo` knows.
struct KeyRow {
    input: Entity<InputState>,
    /// Why the last save, removal or test did not happen.
    error: Option<Text>,
    /// The key test in flight; dropping it cancels the wait for its result.
    testing: Option<Task<()>>,
}

pub struct SettingsScreen {
    bardo: Entity<Bardo>,
    rows: HashMap<Provider, KeyRow>,
    _subscriptions: Vec<Subscription>,
}

impl SettingsScreen {
    pub fn new(bardo: Entity<Bardo>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut subscriptions = vec![cx.observe_in(&bardo, window, |this, _, window, cx| {
            this.relabel(window, cx)
        })];
        let rows = Provider::ALL
            .into_iter()
            .map(|provider| {
                // Masked: a key never shows on screen, even while typed.
                let input = cx.new(|cx| InputState::new(window, cx).masked(true));
                subscriptions.push(cx.subscribe(&input, move |this, _, event, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.row_mut(provider).error = None;
                        cx.notify();
                    }
                }));
                let row = KeyRow {
                    input,
                    error: None,
                    testing: None,
                };
                (provider, row)
            })
            .collect();
        let mut screen = Self {
            bardo,
            rows,
            _subscriptions: subscriptions,
        };
        screen.relabel(window, cx);
        screen
    }

    fn row_mut(&mut self, provider: Provider) -> &mut KeyRow {
        self.rows.get_mut(&provider).expect("a row per provider")
    }

    /// Placeholders follow the interface language.
    fn relabel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for provider in Provider::ALL {
            let placeholder = tr(self.bardo.read(cx), Text::ProviderKeyPlaceholder(provider));
            self.rows[&provider].input.update(cx, |input, cx| {
                input.set_placeholder(placeholder, window, cx)
            });
        }
        cx.notify();
    }

    fn save(&mut self, provider: Provider, window: &mut Window, cx: &mut Context<Self>) {
        let input = self.rows[&provider].input.clone();
        let typed = input.read(cx).value();
        let result = self.bardo.update(cx, |bardo, cx| {
            let result = bardo.save_provider_key(provider, &typed);
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

    fn remove(&mut self, provider: Provider, cx: &mut Context<Self>) {
        let result = self.bardo.update(cx, |bardo, cx| {
            let result = bardo.remove_provider_key(provider);
            cx.notify();
            result
        });
        let row = self.row_mut(provider);
        row.testing = None;
        row.error = result.err().map(|error| error.message());
        cx.notify();
    }

    /// Runs the test on a background thread; the screen stays responsive
    /// and shows the result when it arrives.
    fn test(&mut self, provider: Provider, cx: &mut Context<Self>) {
        let test = match self.bardo.read(cx).key_test(provider) {
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

    fn render_card(&self, status: ProviderKeyStatus, cx: &mut Context<Self>) -> impl IntoElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let provider = status.provider;
        let row = &self.rows[&provider];
        let testing = row.testing.is_some();
        let saved = matches!(status.state, KeyState::Saved { .. });

        let state = match &status.state {
            KeyState::NotSet => tr(bardo, Text::KeyNotSet),
            KeyState::Saved { hint } => bardo.text_with(Text::KeySaved, &[("hint", hint)]).into(),
            KeyState::Unreadable => tr(bardo, Text::KeyUnreadable),
        };
        let state_color = match status.state {
            KeyState::Unreadable => theme.danger,
            _ => theme.muted_foreground,
        };

        let check = status.last_check.filter(|_| !testing).map(|check| {
            let color = outcome_color(check.outcome, theme);
            v_flex()
                .gap_0p5()
                .child(
                    div()
                        .text_sm()
                        .text_color(color)
                        .child(tr(bardo, Text::KeyCheckOutcome(check.outcome))),
                )
                .children(check.detail.map(|detail| {
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(SharedString::from(
                            bardo.text_with(Text::KeyCheckDetail, &[("detail", &detail)]),
                        ))
                }))
        });
        let error = row.error.map(|error| {
            div()
                .text_sm()
                .text_color(theme.danger)
                .child(tr(bardo, error))
        });

        let save_label = if saved {
            Text::ReplaceKey
        } else {
            Text::SaveKey
        };
        let test_label = if testing {
            Text::TestingKey
        } else {
            Text::TestKey
        };

        v_flex()
            .p_4()
            .gap_2()
            .border_1()
            .border_color(theme.border)
            .rounded_lg()
            .child(
                h_flex()
                    .justify_between()
                    .gap_3()
                    .child(
                        div()
                            .font_semibold()
                            .child(tr(bardo, Text::ProviderName(provider))),
                    )
                    .child(div().text_sm().text_color(state_color).child(state)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(tr(bardo, Text::ProviderPurpose(provider))),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(div().flex_1().child(Input::new(&row.input)))
                    .child(
                        Button::new(("save-key", provider as usize))
                            .primary()
                            .label(tr(bardo, save_label))
                            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                this.save(provider, window, cx)
                            })),
                    )
                    .child(
                        Button::new(("test-key", provider as usize))
                            .outline()
                            .label(tr(bardo, test_label))
                            .loading(testing)
                            .disabled(!saved || testing)
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.test(provider, cx)
                            })),
                    )
                    .child(
                        Button::new(("remove-key", provider as usize))
                            .ghost()
                            .label(tr(bardo, Text::RemoveKey))
                            .disabled(status.state == KeyState::NotSet)
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.remove(provider, cx)
                            })),
                    ),
            )
            .children(error)
            .children(check)
    }
}

fn outcome_color(outcome: KeyCheckOutcome, theme: &gpui_kit::component::Theme) -> Hsla {
    match outcome {
        KeyCheckOutcome::Valid => theme.success,
        KeyCheckOutcome::LimitReached
        | KeyCheckOutcome::NotAllowed
        | KeyCheckOutcome::ProviderDown
        | KeyCheckOutcome::Unreachable => theme.warning,
        KeyCheckOutcome::Rejected | KeyCheckOutcome::Unexpected => theme.danger,
    }
}

impl Render for SettingsScreen {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let statuses = self.bardo.read(cx).provider_keys();
        let cards: Vec<_> = statuses
            .into_iter()
            .map(|status| self.render_card(status, cx).into_any_element())
            .collect();
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();

        v_flex()
            .id("settings")
            .size_full()
            .overflow_y_scroll()
            .child(
                v_flex()
                    .max_w(px(760.))
                    .p_6()
                    .gap_4()
                    .child(
                        div()
                            .text_xl()
                            .font_semibold()
                            .child(tr(bardo, Text::SettingsTitle)),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_lg()
                                    .font_medium()
                                    .child(tr(bardo, Text::ProviderKeysTitle)),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme.muted_foreground)
                                    .child(tr(bardo, Text::ProviderKeysHint)),
                            ),
                    )
                    .children(cards),
            )
    }
}
