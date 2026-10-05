//! Settings screen. The API keys tab has one card per provider to save,
//! replace, test and remove its key; the Networks tab keeps the OAuth app
//! credentials Bardo signs in to networks with; the Publishing tab turns the
//! background agent on and off; the Appearance tab picks the layout,
//! the theme and the interface language; the Metrics tab picks when a
//! start syncs public post numbers. Rules, storage and the test call live in
//! `bardo_app`; this file maps clicks to use cases and results to text.

use std::collections::HashMap;

use bardo_app::bardo_domain::{
    AppCredentialsFieldError, KeyCheckOutcome, LayoutId, MetricsSyncOnStart, Network, Provider,
    ThemeFamily, ThemeMode, UiLanguage, UiTheme, UiThemePreference,
};
use bardo_app::{
    AgentStatus, AppCredentialsStatus, Bardo, Control, Destination, KeyState, ProviderKeyStatus,
    SettingsTab, Text, TourAnchor, TourPlace,
};
use gpui_kit::component::button::{Button, ButtonGroup, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::searchable_list::{SearchableListItem, SearchableVec};
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{
    Disableable as _, Icon, IconName, IndexPath, Selectable as _, Sizable as _, StyledExt as _,
    h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Entity, Hsla, MouseButton, ScrollHandle, SharedString,
    Subscription, Task, Window, div, px, rgb,
};

use crate::appearance::{self, look};
use crate::kit::{self, Tone};
use crate::parts::{Header, ScreenParts};
use crate::shell::tr;
use crate::{guide, layout};

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

/// One choice of when a start syncs metrics.
#[derive(Clone)]
struct SyncChoice {
    value: MetricsSyncOnStart,
    title: SharedString,
}

impl SearchableListItem for SyncChoice {
    type Value = MetricsSyncOnStart;

    fn title(&self) -> SharedString {
        self.title.clone()
    }

    fn value(&self) -> &MetricsSyncOnStart {
        &self.value
    }
}

type SyncSelect = Entity<SelectState<SearchableVec<SyncChoice>>>;

fn sync_choices(bardo: &Bardo) -> SearchableVec<SyncChoice> {
    SearchableVec::new(
        MetricsSyncOnStart::ALL
            .map(|value| SyncChoice {
                value,
                title: tr(bardo, Text::MetricsSyncOption(value)),
            })
            .to_vec(),
    )
}

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

/// Field errors each credentials input shows, and clears once edited.
const CLIENT_ID_ERRORS: &[AppCredentialsFieldError] = &[
    AppCredentialsFieldError::ClientIdRequired,
    AppCredentialsFieldError::ClientIdInvalid,
];
const CLIENT_SECRET_ERRORS: &[AppCredentialsFieldError] = &[
    AppCredentialsFieldError::ClientSecretRequired,
    AppCredentialsFieldError::ClientSecretInvalid,
];

/// What the screen holds for one network's app credentials.
struct CredentialsRow {
    client_id: Entity<InputState>,
    client_secret: Entity<InputState>,
    field_errors: Vec<AppCredentialsFieldError>,
    /// Why the last save or removal did not happen.
    error: Option<Text>,
}

pub struct SettingsScreen {
    bardo: Entity<Bardo>,
    tab: SettingsTab,
    rows: HashMap<Provider, KeyRow>,
    credentials: HashMap<Network, CredentialsRow>,
    /// The light and dark slots of "follow Windows".
    light_theme: ThemeSelect,
    dark_theme: ThemeSelect,
    /// When a start syncs metrics.
    metrics_sync: SyncSelect,
    /// Why the last metrics setting was not saved.
    metrics_error: Option<Text>,
    /// Why the last theme or language change was not saved.
    appearance_error: Option<Text>,
    /// Why the background agent did not turn on or off.
    agent_error: Option<Text>,
    /// The agent's task changing with the system; dropping it stops waiting
    /// for the result.
    agent_work: Option<Task<()>>,
    /// What the labels and pickers were last set from; other changes to
    /// Bardo leave them, and an open picker, alone.
    labeled: Option<(UiLanguage, UiThemePreference)>,
    /// The tab's scroll, so the tour brings its parts into view.
    scroll: ScrollHandle,
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
        let credentials = Network::sign_in_networks()
            .map(|network| {
                let client_id = cx.new(|cx| InputState::new(window, cx));
                // Masked like an API key: the secret never shows on screen.
                let client_secret = cx.new(|cx| InputState::new(window, cx).masked(true));
                for (input, fields) in [
                    (&client_id, CLIENT_ID_ERRORS),
                    (&client_secret, CLIENT_SECRET_ERRORS),
                ] {
                    subscriptions.push(cx.subscribe(input, move |this, _, event, cx| {
                        if matches!(event, InputEvent::Change) {
                            let row = this.credentials_mut(network);
                            row.field_errors.retain(|error| !fields.contains(error));
                            row.error = None;
                            cx.notify();
                        }
                    }));
                }
                let row = CredentialsRow {
                    client_id,
                    client_secret,
                    field_errors: Vec::new(),
                    error: None,
                };
                (network, row)
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
        let metrics_sync = {
            let bardo = bardo.read(cx);
            let choices = sync_choices(bardo);
            let at = MetricsSyncOnStart::ALL
                .iter()
                .position(|setting| *setting == bardo.metrics_sync_setting())
                .unwrap_or(0);
            cx.new(|cx| SelectState::new(choices, Some(IndexPath::new(at)), window, cx))
        };
        subscriptions.push(cx.subscribe_in(
            &metrics_sync,
            window,
            |this, _, event: &SelectEvent<SearchableVec<SyncChoice>>, _, cx| {
                if let SelectEvent::Confirm(Some(setting)) = event {
                    this.set_metrics_sync(*setting, cx);
                }
            },
        ));
        let mut screen = Self {
            bardo,
            tab: SettingsTab::Keys,
            rows,
            credentials,
            light_theme,
            dark_theme,
            metrics_sync,
            metrics_error: None,
            appearance_error: None,
            agent_error: None,
            agent_work: None,
            labeled: None,
            scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        };
        screen.relabel(window, cx);
        screen
    }

    fn row_mut(&mut self, provider: Provider) -> &mut KeyRow {
        self.rows.get_mut(&provider).expect("a row per provider")
    }

    fn credentials_mut(&mut self, network: Network) -> &mut CredentialsRow {
        self.credentials
            .get_mut(&network)
            .expect("a row per sign-in network")
    }

    /// The tab on screen.
    pub fn tab(&self) -> SettingsTab {
        self.tab
    }

    pub fn show_tab(&mut self, tab: SettingsTab, cx: &mut Context<Self>) {
        self.tab = tab;
        cx.notify();
    }

    /// Opens the Networks tab, where a connection's app credentials live.
    pub fn show_networks(&mut self, cx: &mut Context<Self>) {
        self.tab = SettingsTab::Networks;
        cx.notify();
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
        for network in Network::sign_in_networks() {
            let bardo = self.bardo.read(cx);
            let id = tr(bardo, Text::ClientIdPlaceholder(network));
            let secret = tr(bardo, Text::ClientSecretPlaceholder(network));
            let row = &self.credentials[&network];
            row.client_id
                .update(cx, |input, cx| input.set_placeholder(id, window, cx));
            row.client_secret
                .update(cx, |input, cx| input.set_placeholder(secret, window, cx));
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
        let bardo = self.bardo.read(cx);
        let (choices, setting) = (sync_choices(bardo), bardo.metrics_sync_setting());
        self.metrics_sync.update(cx, |select, cx| {
            select.set_items(choices, window, cx);
            select.set_selected_value(&setting, window, cx);
        });
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

    /// Saves the layout and arranges every screen in it at once.
    fn set_layout(&mut self, chosen: LayoutId, cx: &mut Context<Self>) {
        let result = self.bardo.update(cx, |bardo, cx| {
            let result = bardo.set_ui_layout(chosen);
            cx.notify();
            result
        });
        match result {
            Ok(()) => {
                self.appearance_error = None;
                layout::show(chosen, cx);
            }
            Err(_) => self.appearance_error = Some(Text::UiLayoutNotSaved),
        }
        cx.notify();
    }

    fn set_metrics_sync(&mut self, setting: MetricsSyncOnStart, cx: &mut Context<Self>) {
        let result = self.bardo.update(cx, |bardo, cx| {
            let result = bardo.set_metrics_sync_on_start(setting);
            cx.notify();
            result
        });
        self.metrics_error = result.err().map(|_| Text::MetricsSettingNotSaved);
        cx.notify();
    }

    /// "Offer tours on new screens": the "new" mark on screens' tours.
    fn set_offer_tours(&mut self, on: bool, cx: &mut Context<Self>) {
        let result = self.bardo.update(cx, |bardo, cx| {
            let result = bardo.set_offer_screen_tours(on);
            cx.notify();
            result
        });
        self.appearance_error = result.err().map(|_| Text::ToursSettingNotSaved);
        cx.notify();
    }

    /// Turns the background agent on or off: the choice is saved at once,
    /// and the system's task changes on a background thread.
    fn set_background_agent(&mut self, on: bool, cx: &mut Context<Self>) {
        let result = self.bardo.update(cx, |bardo, cx| {
            let result = bardo.set_background_agent(on);
            cx.notify();
            result
        });
        match result {
            Ok(work) => self.run_agent_work(work, cx),
            Err(error) => {
                self.agent_error = Some(error.message());
                cx.notify();
            }
        }
    }

    /// Starts the agent that is on but not running.
    fn start_background_agent(&mut self, cx: &mut Context<Self>) {
        let work = self.bardo.read(cx).start_background_agent();
        self.run_agent_work(work, cx);
    }

    fn run_agent_work(&mut self, work: bardo_app::AgentWork, cx: &mut Context<Self>) {
        let bardo = self.bardo.clone();
        let task = cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { work.run() })
                .await;
            let error = match result {
                Ok(()) => None,
                Err(error) => {
                    tracing::warn!("{error}");
                    let message = error.message();
                    // An agent Windows did not set up stays off.
                    if matches!(error, bardo_app::AgentError::NotSetUp(_)) {
                        let undone = bardo.update(cx, |bardo, cx| {
                            let undone = bardo.background_agent_not_set_up();
                            cx.notify();
                            undone
                        });
                        if let Err(error) = undone {
                            tracing::warn!("{error}");
                        }
                    }
                    Some(message)
                }
            };
            let _ = this.update(cx, |this, cx| {
                this.agent_work = None;
                this.agent_error = error;
                cx.notify();
            });
        });
        self.agent_error = None;
        self.agent_work = Some(task);
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

    fn save_credentials(&mut self, network: Network, window: &mut Window, cx: &mut Context<Self>) {
        let row = &self.credentials[&network];
        let (id_input, secret_input) = (row.client_id.clone(), row.client_secret.clone());
        let id = id_input.read(cx).value();
        let secret = secret_input.read(cx).value();
        let result = self.bardo.update(cx, |bardo, cx| {
            let result = bardo.save_app_credentials(network, &id, &secret);
            cx.notify();
            result
        });
        let row = self.credentials_mut(network);
        match result {
            Ok(()) => {
                row.field_errors.clear();
                row.error = None;
                id_input.update(cx, |input, cx| input.set_value("", window, cx));
                secret_input.update(cx, |input, cx| input.set_value("", window, cx));
            }
            Err(error) => {
                row.field_errors = error.field_errors().to_vec();
                row.error = error.message();
            }
        }
        cx.notify();
    }

    fn remove_credentials(&mut self, network: Network, cx: &mut Context<Self>) {
        let result = self.bardo.update(cx, |bardo, cx| {
            let result = bardo.remove_app_credentials(network);
            cx.notify();
            result
        });
        let row = self.credentials_mut(network);
        row.field_errors.clear();
        row.error = result.err().and_then(|error| error.message());
        cx.notify();
    }

    fn render_credentials_card(
        &self,
        status: AppCredentialsStatus,
        first: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let tokens = look(cx).tokens;
        let network = status.network;
        let row = &self.credentials[&network];
        let saved = matches!(status.state, KeyState::Saved { .. });

        let state = match &status.state {
            KeyState::NotSet => kit::status_with(
                Tone::Neutral,
                IconName::Minus,
                tr(bardo, Text::AppCredentialsNotSet),
                cx,
            ),
            KeyState::Saved { hint } => kit::status(
                Tone::Success,
                bardo.text_with(Text::AppCredentialsSaved, &[("hint", hint)]),
                cx,
            ),
            KeyState::Unreadable => {
                kit::status(Tone::Danger, tr(bardo, Text::AppCredentialsUnreadable), cx)
            }
        };
        // The tour shows the first card's state, and the card itself.
        let scroll = Some(&self.scroll);
        let state = if first {
            kit::anchor_in(
                TourAnchor::Control(Control::CredentialsState),
                state,
                scroll,
            )
            .into_any_element()
        } else {
            state.into_any_element()
        };
        let field_error = |fields: &[AppCredentialsFieldError]| {
            let error = row.field_errors.iter().find(|e| fields.contains(e))?;
            Some(
                div()
                    .text_xs()
                    .text_color(tokens.danger)
                    .child(tr(bardo, Text::AppCredentialsFieldError(network, *error))),
            )
        };
        let field = |label: Text, input: &Entity<InputState>, fields| {
            v_flex()
                .flex_1()
                .min_w(px(240.))
                .gap_1()
                .child(div().text_sm().font_medium().child(tr(bardo, label)))
                .child(Input::new(input))
                .children(field_error(fields))
        };
        let save = Button::new(("save-credentials", network as usize))
            .label(tr(
                bardo,
                if saved {
                    Text::ReplaceAppCredentials
                } else {
                    Text::SaveAppCredentials
                },
            ))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.save_credentials(network, window, cx)
            }));
        let save = if saved {
            save.outline()
        } else {
            save.primary()
        };
        let error = row
            .error
            .map(|error| kit::notice(Tone::Danger, tr(bardo, error), cx));

        let card =
            kit::card(cx)
                .p_4()
                .gap_3()
                .child(
                    h_flex()
                        .flex_wrap()
                        .justify_between()
                        .gap_3()
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w(px(240.))
                                .gap_0p5()
                                .child(
                                    div()
                                        .font_semibold()
                                        .child(tr(bardo, Text::AppCredentialsName(network))),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(tokens.text2)
                                        .child(tr(bardo, Text::AppCredentialsPurpose(network))),
                                ),
                        )
                        .child(state),
                )
                .child(
                    h_flex()
                        .flex_wrap()
                        .items_start()
                        .gap_3()
                        .child(field(
                            Text::ClientId(network),
                            &row.client_id,
                            CLIENT_ID_ERRORS,
                        ))
                        .child(field(
                            Text::ClientSecret(network),
                            &row.client_secret,
                            CLIENT_SECRET_ERRORS,
                        )),
                )
                .child(h_flex().gap_2().child(save).when(
                    status.state != KeyState::NotSet,
                    |actions| {
                        actions.child(
                            Button::new(("remove-credentials", network as usize))
                                .ghost()
                                .label(tr(bardo, Text::RemoveAppCredentials))
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.remove_credentials(network, cx)
                                })),
                        )
                    },
                ))
                .children(error);
        if first {
            kit::anchor_in(TourAnchor::Control(Control::CredentialsCard), card, scroll)
                .into_any_element()
        } else {
            card.into_any_element()
        }
    }

    fn render_networks(&self, cx: &mut Context<Self>) -> AnyElement {
        let statuses = self.bardo.read(cx).app_credentials();
        let cards: Vec<_> = statuses
            .into_iter()
            .enumerate()
            .map(|(index, status)| self.render_credentials_card(status, index == 0, cx))
            .collect();
        let bardo = self.bardo.read(cx);
        v_flex()
            .gap_3()
            .child(kit::anchor_in(
                TourAnchor::Control(Control::CredentialsWhy),
                kit::section_heading(tr(bardo, Text::AppCredentialsTitle)).child(
                    guide::labeled_info(
                        bardo,
                        "app-credentials-info",
                        Some(tr(bardo, Text::AppCredentialsInfo)),
                        tr(bardo, Text::AppCredentialsHint),
                        guide::refs::CREDENTIALS_KEPT,
                    ),
                ),
                Some(&self.scroll),
            ))
            .children(cards)
            .into_any_element()
    }

    /// A provider's card. The tour lights the first card (`first`), and the
    /// state and Test key of the first card with a saved key
    /// (`first_saved`).
    fn render_card(
        &self,
        status: ProviderKeyStatus,
        first: bool,
        first_saved: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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
        let scroll = Some(&self.scroll);
        let state = if first_saved {
            kit::anchor_in(TourAnchor::Control(Control::KeyState), state, scroll).into_any_element()
        } else {
            state.into_any_element()
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

        let card = kit::card(cx)
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
                        let test = Button::new(("test-key", provider as usize))
                            .outline()
                            .label(tr(bardo, test_label))
                            .loading(testing)
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                if this.rows[&provider].testing.is_none() {
                                    this.test(provider, cx)
                                }
                            }));
                        if first_saved {
                            actions.child(kit::anchor_in(
                                TourAnchor::Control(Control::KeyTest),
                                test,
                                scroll,
                            ))
                        } else {
                            actions.child(test)
                        }
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
            .children(check);
        if first {
            kit::anchor_in(TourAnchor::Control(Control::KeyCard), card, scroll).into_any_element()
        } else {
            card.into_any_element()
        }
    }

    fn render_keys(&self, cx: &mut Context<Self>) -> AnyElement {
        let statuses = self.bardo.read(cx).provider_keys();
        let first_saved = statuses
            .iter()
            .position(|status| matches!(status.state, KeyState::Saved { .. }));
        let cards: Vec<_> = statuses
            .into_iter()
            .enumerate()
            .map(|(index, status)| {
                self.render_card(status, index == 0, Some(index) == first_saved, cx)
            })
            .collect();
        let bardo = self.bardo.read(cx);
        v_flex()
            .gap_3()
            .child(kit::anchor_in(
                TourAnchor::Control(Control::KeysKept),
                kit::section_heading(tr(bardo, Text::ProviderKeysTitle)).child(
                    guide::labeled_info(
                        bardo,
                        "provider-keys-info",
                        Some(tr(bardo, Text::ProviderKeysInfo)),
                        tr(bardo, Text::ProviderKeysHint),
                        guide::refs::KEYS_KEPT,
                    ),
                ),
                Some(&self.scroll),
            ))
            .children(cards)
            .into_any_element()
    }

    fn render_appearance(&self, cx: &mut Context<Self>) -> AnyElement {
        let chosen_layout = self.bardo.read(cx).ui_layout();
        let layouts: Vec<AnyElement> = LayoutId::ALL
            .into_iter()
            .map(|layout| self.layout_card(layout, layout == chosen_layout, cx))
            .collect();
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

        // Each part the tour lights, in the page's scroll.
        let scroll = Some(&self.scroll);
        let part = |anchor: TourAnchor, children: Vec<AnyElement>| {
            kit::anchor_in(anchor, v_flex().gap_3().children(children), scroll)
        };
        let layout_part = part(
            TourAnchor::Control(Control::AppearanceLayout),
            vec![
                kit::section_heading(tr(bardo, Text::AppearanceLayout))
                    .child(guide::info(
                        bardo,
                        "layout-info",
                        tr(bardo, Text::AppearanceLayoutHint),
                        guide::refs::SETTINGS_LAYOUT,
                    ))
                    .into_any_element(),
                h_flex()
                    .flex_wrap()
                    .items_stretch()
                    .gap_3()
                    .children(layouts)
                    .into_any_element(),
            ],
        );
        let follow_part = part(
            TourAnchor::Control(Control::AppearanceFollow),
            vec![
                kit::section_heading(tr(bardo, Text::AppearanceTheme)).into_any_element(),
                follow.into_any_element(),
            ],
        );
        let theme_part = part(
            TourAnchor::Control(Control::AppearanceTheme),
            vec![
                always.into_any_element(),
                h_flex()
                    .flex_wrap()
                    .gap_3()
                    .children(cards)
                    .into_any_element(),
            ],
        );
        let language_part = part(
            TourAnchor::Control(Control::AppearanceLanguage),
            vec![
                kit::section_heading(tr(bardo, Text::UiLanguageLabel)).into_any_element(),
                h_flex().child(language_switch).into_any_element(),
            ],
        );
        let tours_part = part(
            TourAnchor::Control(Control::AppearanceTours),
            vec![
                kit::section_heading(tr(bardo, Text::ToursSettingTitle)).into_any_element(),
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        Checkbox::new("offer-screen-tours")
                            .label(tr(bardo, Text::ToursSettingOffer))
                            .checked(bardo.offers_screen_tours())
                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                this.set_offer_tours(*checked, cx);
                            })),
                    )
                    .child(guide::info(
                        bardo,
                        "offer-screen-tours-info",
                        tr(bardo, Text::ToursSettingHint),
                        guide::refs::SETTINGS_TOURS,
                    ))
                    .into_any_element(),
            ],
        );

        v_flex()
            .gap_3()
            .children(
                self.appearance_error
                    .map(|error| kit::notice(Tone::Danger, tr(bardo, error), cx)),
            )
            .child(layout_part)
            .child(div().h_2())
            .child(follow_part)
            .child(theme_part)
            .child(div().h_2())
            .child(language_part)
            .child(div().h_2())
            .child(tours_part)
            .into_any_element()
    }

    fn render_publishing(&self, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let t = look(cx).tokens;
        let scroll = Some(&self.scroll);
        let working = self.agent_work.is_some();
        let status = bardo.background_agent();
        let (tone, label) = match status {
            AgentStatus::Off => (Tone::Neutral, Text::AgentStatusOff),
            AgentStatus::Running => (Tone::Success, Text::AgentStatusRunning),
            AgentStatus::NotRunning => (Tone::Warning, Text::AgentStatusNotRunning),
        };
        let switch = kit::anchor_in(
            TourAnchor::Control(Control::AgentSwitch),
            v_flex()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .font_medium()
                        .child(tr(bardo, Text::AgentQuestion)),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(t.text2)
                        .child(tr(bardo, Text::AgentExplanation)),
                )
                .child(
                    h_flex()
                        .pt_1()
                        .gap_1()
                        .items_center()
                        .child(
                            Checkbox::new("background-agent")
                                .label(tr(bardo, Text::AgentSwitch))
                                .checked(status != AgentStatus::Off)
                                .disabled(working)
                                .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                    this.set_background_agent(*checked, cx);
                                })),
                        )
                        .child(guide::info(
                            bardo,
                            "background-agent-info",
                            tr(bardo, Text::AgentHint),
                            guide::refs::AGENT_ON,
                        )),
                ),
            scroll,
        );
        let state = if working {
            kit::notice(Tone::Info, tr(bardo, Text::AgentWorking), cx)
        } else {
            kit::notice(tone, tr(bardo, label), cx)
        };
        let status_part = kit::anchor_in(
            TourAnchor::Control(Control::AgentStatus),
            h_flex()
                .flex_wrap()
                .gap_3()
                .items_center()
                .child(state)
                .when(status == AgentStatus::NotRunning && !working, |row| {
                    row.child(
                        Button::new("start-background-agent")
                            .small()
                            .label(tr(bardo, Text::AgentStartNow))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.start_background_agent(cx);
                            })),
                    )
                }),
            scroll,
        );
        let note = |text: Text| div().text_xs().text_color(t.text2).child(tr(bardo, text));
        v_flex()
            .max_w(px(720.))
            .gap_3()
            .children(
                self.agent_error
                    .map(|error| kit::notice(Tone::Danger, tr(bardo, error), cx)),
            )
            .child(kit::section_heading(tr(bardo, Text::AgentTitle)))
            .child(switch)
            .child(status_part)
            .child(div().h_1())
            .child(note(Text::AgentSends))
            .child(note(Text::AgentLimits))
            .into_any_element()
    }

    fn render_metrics(&self, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        v_flex()
            .gap_3()
            .children(
                self.metrics_error
                    .map(|error| kit::notice(Tone::Danger, tr(bardo, error), cx)),
            )
            .child(kit::anchor_in(
                TourAnchor::Control(Control::MetricsSync),
                kit::field(
                    tr(bardo, Text::MetricsSettingLabel),
                    Some(guide::info(
                        bardo,
                        "metrics-sync-info",
                        tr(bardo, Text::MetricsSettingHint),
                        guide::refs::METRICS_ON_START,
                    )),
                    div()
                        .w(px(280.))
                        .child(Select::new(&self.metrics_sync).small())
                        .into_any_element(),
                    Some(
                        div()
                            .text_xs()
                            .text_color(look(cx).tokens.text2)
                            .child(tr(bardo, Text::MetricsSyncHint))
                            .into_any_element(),
                    ),
                ),
                Some(&self.scroll),
            ))
            .into_any_element()
    }

    /// A layout as a sketch of where things go, its name and one line.
    fn layout_card(&self, layout: LayoutId, chosen: bool, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let t = look(cx).tokens;
        let block = |color: Hsla| div().rounded(t.radius).bg(color);
        let rows = |count: usize| {
            v_flex().gap_1().children((0..count).map(|ix| {
                h_flex()
                    .gap_1()
                    .child(block(t.text2).w(px(10.)).h(px(6.)))
                    .child(
                        block(if ix == 1 { t.accent } else { t.border_strong })
                            .flex_1()
                            .h(px(6.)),
                    )
            }))
        };
        let sketch = match layout {
            LayoutId::Workspace => h_flex()
                .size_full()
                .gap_1p5()
                .child(
                    v_flex()
                        .w(px(34.))
                        .h_full()
                        .gap_1()
                        .p_1()
                        .bg(t.surface)
                        .child(block(t.accent).w_full().h(px(5.)))
                        .child(block(t.border_strong).w_full().h(px(5.)))
                        .child(block(t.border_strong).w_full().h(px(5.))),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .h_full()
                        .gap_1()
                        .py_1()
                        .child(block(t.border_strong).w(px(60.)).h(px(6.)))
                        .child(div().grid().grid_cols(3).gap_1().children((0..6).map(|ix| {
                            block(if ix == 2 { t.accent } else { t.sunken })
                                .h(px(18.))
                                .border(t.border_width)
                                .border_color(t.border)
                        }))),
                )
                .child(block(t.surface).w(px(40.)).h_full())
                .into_any_element(),
            LayoutId::Studio => v_flex()
                .size_full()
                .gap_1()
                .child(
                    h_flex()
                        .gap_1()
                        .p_1()
                        .bg(t.surface)
                        .child(block(t.accent).w(px(14.)).h(px(5.)))
                        .children((0..4).map(|_| block(t.border_strong).w(px(18.)).h(px(5.)))),
                )
                .child(div().flex_1().px_1().child(rows(3)))
                .child(
                    h_flex()
                        .h(px(20.))
                        .gap_1()
                        .p_1()
                        .bg(t.surface)
                        .child(block(t.sunken).flex_1().h_full())
                        .child(block(t.sunken).flex_1().h_full())
                        .child(block(t.sunken).w(px(24.)).h_full()),
                )
                .into_any_element(),
        };
        v_flex()
            .id(("layout-card", layout as usize))
            .w(px(260.))
            .overflow_hidden()
            .rounded(t.radius_lg)
            .border(t.border_width)
            .border_color(if chosen { t.accent } else { t.frame })
            .when(chosen, |card| card.border_2())
            .bg(t.surface)
            .cursor_pointer()
            .hover(|card| card.border_color(t.accent_edge))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.set_layout(layout, cx)))
            .child(div().h(px(84.)).p_2().bg(t.app).child(sketch))
            .child(
                v_flex()
                    .px_2p5()
                    .py_2()
                    .gap_0p5()
                    .border_t_1()
                    .border_color(t.border)
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(radio(chosen, cx))
                            .child(
                                div()
                                    .text_sm()
                                    .font_medium()
                                    .child(tr(bardo, Text::UiLayoutName(layout))),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(t.text2)
                            .child(tr(bardo, Text::UiLayoutDescription(layout))),
                    ),
            )
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
            SettingsTab::Networks => self.render_networks(cx),
            SettingsTab::Publishing => self.render_publishing(cx),
            SettingsTab::Appearance => self.render_appearance(cx),
            SettingsTab::Metrics => self.render_metrics(cx),
        };
        let bardo = self.bardo.read(cx);
        let tabs = TabBar::new("settings-tabs")
            .underline()
            .selected_index(
                SettingsTab::ALL
                    .iter()
                    .position(|tab| *tab == self.tab)
                    .unwrap_or(0),
            )
            .child(Tab::new().label(tr(bardo, Text::SettingsKeysTab)))
            .child(Tab::new().label(tr(bardo, Text::SettingsNetworksTab)))
            .child(Tab::new().label(tr(bardo, Text::SettingsPublishingTab)))
            .child(Tab::new().label(tr(bardo, Text::SettingsAppearanceTab)))
            .child(Tab::new().label(tr(bardo, Text::MetricsSettingsTab)))
            .on_click(cx.listener(|this, index: &usize, _, cx| {
                if let Some(tab) = SettingsTab::ALL.get(*index) {
                    this.tab = *tab;
                    cx.notify();
                }
            }));

        let mut header = Header::place(bardo, Destination::Settings);
        header.info = guide::place_info(bardo, TourPlace::Settings(self.tab), None, cx);
        let mut parts = ScreenParts::new(header);
        parts.toolbar = Some(tabs.into_any_element());
        parts.content = vec![body];
        parts.scroll = Some(self.scroll.clone());
        layout::screen(parts, cx)
    }
}
