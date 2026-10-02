//! Settings screen. The API keys tab has one card per provider to save,
//! replace, test and remove its key; the Appearance tab picks the theme and
//! the interface language. Rules, storage and the test call live in
//! `bardo_app`; this file maps clicks to use cases and results to text.

use std::collections::HashMap;

use bardo_app::bardo_domain::{
    KeyCheckOutcome, Provider, ThemeFamily, ThemeMode, UiLanguage, UiTheme, UiThemePreference,
};
use bardo_app::{Bardo, KeyState, ProviderKeyStatus, Text};
use gpui_kit::component::button::{Button, ButtonGroup, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::searchable_list::{SearchableListItem, SearchableVec};
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{
    Icon, IconName, IndexPath, Selectable as _, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Entity, Hsla, MouseButton, SharedString, Subscription, Task,
    Window, div, px, rgb,
};

use crate::appearance::{self, look};
use crate::kit::{self, Tone};
use crate::shell::tr;

/// The settings tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    Keys,
    Appearance,
}

/// One theme in a light or dark picker.
#[derive(Clone)]
struct ThemeChoice {
    value: UiTheme,
    title: SharedString,
}

impl SearchableListItem for ThemeChoice {
    type Value = UiTheme;

    fn title(&self) -> SharedString {
        self.title.clone()
    }

    fn value(&self) -> &UiTheme {
        &self.value
    }
}

type ThemeSelect = Entity<SelectState<SearchableVec<ThemeChoice>>>;

fn theme_choices(bardo: &Bardo, mode: ThemeMode) -> SearchableVec<ThemeChoice> {
    SearchableVec::new(
        UiTheme::of_mode(mode)
            .map(|value| ThemeChoice {
                value,
                title: tr(bardo, Text::UiThemeName(value)),
            })
            .collect::<Vec<_>>(),
    )
}

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
    tab: SettingsTab,
    rows: HashMap<Provider, KeyRow>,
    /// The light and dark slots of "follow Windows".
    light_theme: ThemeSelect,
    dark_theme: ThemeSelect,
    /// Why the last theme or language change was not saved.
    appearance_error: Option<Text>,
    /// What the labels and pickers were last set from; other changes to
    /// Bardo leave them, and an open picker, alone.
    labeled: Option<(UiLanguage, UiThemePreference)>,
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
        let (light, dark) = follow_pair(bardo.read(cx).ui_theme());
        let mut select = |mode, theme: UiTheme| {
            let choices = theme_choices(bardo.read(cx), mode);
            let at = UiTheme::of_mode(mode).position(|t| t == theme).unwrap_or(0);
            cx.new(|cx| SelectState::new(choices, Some(IndexPath::new(at)), window, cx))
        };
        let light_theme = select(ThemeMode::Light, light);
        let dark_theme = select(ThemeMode::Dark, dark);
        for (select, mode) in [
            (&light_theme, ThemeMode::Light),
            (&dark_theme, ThemeMode::Dark),
        ] {
            subscriptions.push(cx.subscribe_in(
                select,
                window,
                move |this, _, event: &SelectEvent<SearchableVec<ThemeChoice>>, window, cx| {
                    let SelectEvent::Confirm(Some(theme)) = event else {
                        return;
                    };
                    let (light, dark) = follow_pair(this.bardo.read(cx).ui_theme());
                    let preference = match mode {
                        ThemeMode::Light => UiThemePreference::FollowSystem {
                            light: *theme,
                            dark,
                        },
                        ThemeMode::Dark => UiThemePreference::FollowSystem {
                            light,
                            dark: *theme,
                        },
                    };
                    this.set_theme(preference, window, cx);
                },
            ));
        }
        let mut screen = Self {
            bardo,
            tab: SettingsTab::Keys,
            rows,
            light_theme,
            dark_theme,
            appearance_error: None,
            labeled: None,
            _subscriptions: subscriptions,
        };
        screen.relabel(window, cx);
        screen
    }

    fn row_mut(&mut self, provider: Provider) -> &mut KeyRow {
        self.rows.get_mut(&provider).expect("a row per provider")
    }

    /// Placeholders and theme names follow the interface language.
    fn relabel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let bardo = self.bardo.read(cx);
        let now = (bardo.ui_language(), bardo.ui_theme());
        if self.labeled == Some(now) {
            return;
        }
        self.labeled = Some(now);
        for provider in Provider::ALL {
            let placeholder = tr(self.bardo.read(cx), Text::ProviderKeyPlaceholder(provider));
            self.rows[&provider].input.update(cx, |input, cx| {
                input.set_placeholder(placeholder, window, cx)
            });
        }
        let (light, dark) = follow_pair(self.bardo.read(cx).ui_theme());
        for (select, mode, theme) in [
            (self.light_theme.clone(), ThemeMode::Light, light),
            (self.dark_theme.clone(), ThemeMode::Dark, dark),
        ] {
            let choices = theme_choices(self.bardo.read(cx), mode);
            select.update(cx, |select, cx| {
                select.set_items(choices, window, cx);
                select.set_selected_value(&theme, window, cx);
            });
        }
        cx.notify();
    }

    /// Saves how the theme is picked and shows the theme it picks now.
    fn set_theme(
        &mut self,
        preference: UiThemePreference,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let result = self.bardo.update(cx, |bardo, cx| {
            let result = bardo.set_ui_theme(preference);
            cx.notify();
            result
        });
        match result {
            Ok(()) => {
                self.appearance_error = None;
                appearance::follow(preference, appearance::system_mode(window), cx);
            }
            Err(_) => self.appearance_error = Some(Text::UiThemeNotSaved),
        }
        cx.notify();
    }

    fn set_language(&mut self, language: UiLanguage, cx: &mut Context<Self>) {
        let result = self.bardo.update(cx, |bardo, cx| {
            let result = bardo.set_ui_language(language);
            cx.notify();
            result
        });
        self.appearance_error = result.err().map(|_| Text::LanguageNotSaved);
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
        let tokens = look(cx).tokens;
        let provider = status.provider;
        let row = &self.rows[&provider];
        let testing = row.testing.is_some();
        let saved = matches!(status.state, KeyState::Saved { .. });

        let state = match &status.state {
            KeyState::NotSet => kit::status_with(
                Tone::Neutral,
                IconName::Minus,
                tr(bardo, Text::KeyNotSet),
                cx,
            ),
            KeyState::Saved { hint } => kit::status(
                Tone::Success,
                bardo.text_with(Text::KeySaved, &[("hint", hint)]),
                cx,
            ),
            KeyState::Unreadable => kit::status(Tone::Danger, tr(bardo, Text::KeyUnreadable), cx),
        };

        let check = status.last_check.filter(|_| !testing).map(|check| {
            h_flex()
                .gap_2()
                .flex_wrap()
                .child(kit::status(
                    outcome_tone(check.outcome),
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

        let test_label = if testing {
            Text::TestingKey
        } else {
            Text::TestKey
        };
        // No key yet: saving is the one thing to do here. With a key,
        // replacing and testing are equals, and removing stays quiet.
        let save =
            Button::new(("save-key", provider as usize))
                .label(tr(
                    bardo,
                    if saved {
                        Text::ReplaceKey
                    } else {
                        Text::SaveKey
                    },
                ))
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.save(provider, window, cx)
                }));
        let save = if saved {
            save.outline()
        } else {
            save.primary()
        };

        kit::card(cx)
            .p_4()
            .gap_2()
            .child(
                h_flex()
                    .justify_between()
                    .gap_3()
                    .child(
                        v_flex()
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
                    .child(state),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(div().flex_1().child(Input::new(&row.input)))
                    .child(save)
                    // Hidden, not disabled, until there is a key to act on.
                    .when(saved, |actions| {
                        actions.child(
                            Button::new(("test-key", provider as usize))
                                .outline()
                                .label(tr(bardo, test_label))
                                .loading(testing)
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    if this.rows[&provider].testing.is_none() {
                                        this.test(provider, cx)
                                    }
                                })),
                        )
                    })
                    .when(status.state != KeyState::NotSet, |actions| {
                        actions.child(
                            Button::new(("remove-key", provider as usize))
                                .ghost()
                                .label(tr(bardo, Text::RemoveKey))
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.remove(provider, cx)
                                })),
                        )
                    }),
            )
            .children(error)
            .children(check)
    }

    fn render_keys(&self, cx: &mut Context<Self>) -> AnyElement {
        let statuses = self.bardo.read(cx).provider_keys();
        let cards: Vec<_> = statuses
            .into_iter()
            .map(|status| self.render_card(status, cx).into_any_element())
            .collect();
        let bardo = self.bardo.read(cx);
        v_flex()
            .gap_3()
            .child(
                kit::section_heading(tr(bardo, Text::ProviderKeysTitle)).child(kit::info(
                    "provider-keys-info",
                    Some(tr(bardo, Text::ProviderKeysInfo)),
                    tr(bardo, Text::ProviderKeysHint),
                )),
            )
            .children(cards)
            .into_any_element()
    }

    fn render_appearance(&self, cx: &mut Context<Self>) -> AnyElement {
        let cards: Vec<AnyElement> = {
            let fixed = match self.bardo.read(cx).ui_theme() {
                UiThemePreference::Fixed(theme) => Some(theme),
                UiThemePreference::FollowSystem { .. } => None,
            };
            UiTheme::ALL
                .into_iter()
                .map(|theme| self.theme_card(theme, fixed == Some(theme), cx))
                .collect()
        };
        let bardo = self.bardo.read(cx);
        let tokens = look(cx).tokens;
        let preference = bardo.ui_theme();
        let following = matches!(preference, UiThemePreference::FollowSystem { .. });

        let option = |id: &'static str, on: bool, title: Text, hint: Text, cx: &App| {
            let t = look(cx).tokens;
            // Wraps the follow row's pickers under its text when the window
            // (or a monospace theme font) leaves no room beside it.
            h_flex()
                .id(id)
                .flex_wrap()
                .gap_3()
                .p_3()
                .rounded(t.radius_lg)
                .border(t.border_width)
                .border_color(if on { t.accent_edge } else { t.frame })
                .bg(if on { t.selected } else { t.surface })
                .cursor_pointer()
                .child(radio(on, cx))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w(px(240.))
                        .child(div().font_medium().child(tr(bardo, title)))
                        .child(div().text_xs().text_color(t.text2).child(tr(bardo, hint))),
                )
        };

        let follow = option(
            "theme-follow",
            following,
            Text::AppearanceFollowSystem,
            Text::AppearanceFollowSystemHint,
            cx,
        )
        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
            let preference = this.bardo.read(cx).ui_theme();
            if let UiThemePreference::Fixed(_) = preference {
                let (light, dark) = follow_pair(preference);
                this.set_theme(UiThemePreference::FollowSystem { light, dark }, window, cx);
            }
        }))
        .child(
            h_flex()
                .gap_2()
                // Opening a picker is not a click on the row: from a fixed
                // theme it would switch to following Windows at once.
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .text_sm()
                        .text_color(tokens.text2)
                        .child(tr(bardo, Text::AppearanceLight)),
                )
                .child(
                    div()
                        .w(px(200.))
                        .child(Select::new(&self.light_theme).small()),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(tokens.text2)
                        .child(tr(bardo, Text::AppearanceDark)),
                )
                .child(
                    div()
                        .w(px(200.))
                        .child(Select::new(&self.dark_theme).small()),
                ),
        );

        let always = option(
            "theme-fixed",
            !following,
            Text::AppearanceFixed,
            Text::AppearanceFixedHint,
            cx,
        )
        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
            if matches!(
                this.bardo.read(cx).ui_theme(),
                UiThemePreference::FollowSystem { .. }
            ) {
                let current = look(cx).theme;
                this.set_theme(UiThemePreference::Fixed(current), window, cx);
            }
        }));

        let current_language = bardo.ui_language();
        let language_switch = ButtonGroup::new("ui-language")
            .outline()
            .children(UiLanguage::ALL.map(|language| {
                Button::new(language.tag())
                    .label(tr(bardo, Text::LanguageName(language)))
                    .selected(language == current_language)
            }))
            .on_click(cx.listener(|this, clicked: &Vec<usize>, _, cx| {
                if let Some(language) = clicked.first().and_then(|&i| UiLanguage::ALL.get(i)) {
                    this.set_language(*language, cx);
                }
            }));

        v_flex()
            .gap_3()
            .children(
                self.appearance_error
                    .map(|error| kit::notice(Tone::Danger, tr(bardo, error), cx)),
            )
            .child(kit::section_heading(tr(bardo, Text::AppearanceTheme)))
            .child(follow)
            .child(always)
            .child(h_flex().flex_wrap().gap_3().children(cards))
            .child(div().h_2())
            .child(kit::section_heading(tr(bardo, Text::UiLanguageLabel)))
            .child(h_flex().child(language_switch))
            .into_any_element()
    }

    /// A theme in its own colors: a small preview, its name and kind, and
    /// the contrast level its text meets.
    fn theme_card(&self, theme: UiTheme, chosen: bool, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let t = look(cx).tokens;
        let p = bardo_app::palette(theme);
        let c = |value: bardo_app::Rgb| -> Hsla { rgb(value.0).into() };
        let radius = px(f32::from(p.radius));
        let bar = |width: f32, color: Hsla| div().h(px(5.)).w(px(width)).rounded(radius).bg(color);
        let preview = h_flex()
            .h(px(70.))
            .bg(c(p.app))
            .child(
                v_flex()
                    .w(px(40.))
                    .h_full()
                    .gap_1()
                    .p_2()
                    .bg(c(p.surface))
                    .border_r_1()
                    .border_color(c(p.border))
                    .child(bar(22., c(p.text3)))
                    .child(bar(22., c(p.accent)))
                    .child(bar(22., c(p.text3))),
            )
            .child(
                v_flex()
                    .flex_1()
                    .gap_1()
                    .p_2()
                    .child(bar(46., c(p.text)))
                    .child(bar(80., c(p.text2)))
                    .child(bar(60., c(p.text3)))
                    .child(
                        h_flex()
                            .gap_1p5()
                            .mt_1()
                            .child(
                                div()
                                    .h(px(12.))
                                    .w(px(36.))
                                    .rounded(radius)
                                    .bg(c(p.accent))
                                    .border_1()
                                    .border_color(c(p.accent_edge)),
                            )
                            .child(
                                div()
                                    .h(px(12.))
                                    .w(px(28.))
                                    .rounded(radius)
                                    .bg(c(p.success_bg))
                                    .border_1()
                                    .border_color(c(p.success)),
                            ),
                    ),
            );
        let level = if theme.family() == ThemeFamily::HighContrast {
            "AAA"
        } else {
            "AA"
        };
        let contrast = bardo.text_with(Text::AppearanceContrast, &[("level", level)]);
        v_flex()
            .id(("theme-card", theme as usize))
            .w(px(186.))
            .overflow_hidden()
            .rounded(t.radius_lg)
            .border(t.border_width)
            .border_color(if chosen { t.accent } else { t.frame })
            .when(chosen, |card| card.border_2())
            .bg(t.surface)
            .cursor_pointer()
            .hover(|card| card.border_color(t.accent_edge))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.set_theme(UiThemePreference::Fixed(theme), window, cx)
            }))
            .child(preview)
            .child(
                v_flex()
                    .px_2p5()
                    .py_2()
                    .border_t_1()
                    .border_color(t.border)
                    .child(
                        div()
                            .text_sm()
                            .font_medium()
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_ellipsis()
                            .child(tr(bardo, Text::UiThemeName(theme))),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .justify_between()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(t.text2)
                                    .child(tr(bardo, Text::UiThemeKind(theme))),
                            )
                            .child(
                                h_flex()
                                    .id(("theme-contrast", theme as usize))
                                    .gap_0p5()
                                    .text_xs()
                                    .text_color(t.success)
                                    .child(Icon::new(IconName::Check).size(px(12.)))
                                    .child(level)
                                    .tooltip(move |window, cx| {
                                        gpui_kit::component::tooltip::Tooltip::new(contrast.clone())
                                            .build(window, cx)
                                    }),
                            ),
                    ),
            )
            .into_any_element()
    }
}

/// The light and dark themes "follow Windows" uses: the saved pair, or
/// the default pair with the fixed theme in its own slot.
fn follow_pair(preference: UiThemePreference) -> (UiTheme, UiTheme) {
    let (light, dark) = match UiThemePreference::default() {
        UiThemePreference::FollowSystem { light, dark } => (light, dark),
        UiThemePreference::Fixed(theme) => (theme, theme),
    };
    match preference {
        UiThemePreference::FollowSystem { light, dark } => (light, dark),
        UiThemePreference::Fixed(theme) => match theme.mode() {
            ThemeMode::Light => (theme, dark),
            ThemeMode::Dark => (light, theme),
        },
    }
}

fn radio(on: bool, cx: &App) -> impl IntoElement {
    let t = look(cx).tokens;
    div()
        .flex_none()
        .size(px(16.))
        .rounded_full()
        .bg(t.raised)
        .border_color(if on { t.accent } else { t.border_strong })
        .map(|dot| {
            if on {
                dot.border(px(5.))
            } else {
                dot.border_1()
            }
        })
}

fn outcome_tone(outcome: KeyCheckOutcome) -> Tone {
    match outcome {
        KeyCheckOutcome::Valid => Tone::Success,
        KeyCheckOutcome::LimitReached
        | KeyCheckOutcome::NotAllowed
        | KeyCheckOutcome::ProviderDown
        | KeyCheckOutcome::Unreachable => Tone::Warning,
        KeyCheckOutcome::Rejected | KeyCheckOutcome::Unexpected => Tone::Danger,
    }
}

impl Render for SettingsScreen {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match self.tab {
            SettingsTab::Keys => self.render_keys(cx),
            SettingsTab::Appearance => self.render_appearance(cx),
        };
        let bardo = self.bardo.read(cx);
        let tabs = TabBar::new("settings-tabs")
            .underline()
            .selected_index(match self.tab {
                SettingsTab::Keys => 0,
                SettingsTab::Appearance => 1,
            })
            .child(Tab::new().label(tr(bardo, Text::SettingsKeysTab)))
            .child(Tab::new().label(tr(bardo, Text::SettingsAppearanceTab)))
            .on_click(cx.listener(|this, index: &usize, _, cx| {
                this.tab = if *index == 0 {
                    SettingsTab::Keys
                } else {
                    SettingsTab::Appearance
                };
                cx.notify();
            }));

        v_flex()
            .id("settings")
            .size_full()
            .overflow_y_scroll()
            .child(
                v_flex()
                    .max_w(px(1040.))
                    .p_6()
                    .gap_4()
                    .child(kit::title(tr(bardo, Text::SettingsTitle)))
                    .child(tabs)
                    .child(body),
            )
    }
}
