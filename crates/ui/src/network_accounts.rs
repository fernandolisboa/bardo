//! The network accounts of the channel being edited, shown under the
//! channel form: one row per account, buttons to add the networks still
//! free, and an inline form for the account being added or edited. Rules
//! and persistence live in `bardo_app`; this file only maps the form to a
//! `NetworkAccountDraft` and errors back to fields.

use bardo_app::bardo_domain::{
    AspectRatio, ChannelId, ContentLanguage, Network, NetworkAccount, NetworkAccountDraft,
    NetworkAccountFieldError, NetworkAccountId, Resolution, VideoCodec, Visibility,
};
use bardo_app::{Bardo, NetworkAccountError, Text};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::searchable_list::{SearchableListItem, SearchableVec};
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::{
    ActiveTheme as _, IconName, IndexPath, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, ClickEvent, Entity, SharedString, Subscription, Window, div};

use crate::appearance::look;
use crate::kit::{self, Tone};
use crate::shell::tr;

/// One option of a select: a domain value and its label.
#[derive(Clone)]
struct Choice<T> {
    value: T,
    title: SharedString,
}

impl<T: Clone + PartialEq> SearchableListItem for Choice<T> {
    type Value = T;

    fn title(&self) -> SharedString {
        self.title.clone()
    }

    fn value(&self) -> &T {
        &self.value
    }
}

type Choices<T> = SearchableVec<Choice<T>>;
type ChoiceSelect<T> = Entity<SelectState<Choices<T>>>;

/// "value (network default)" first, then every value. `None` keeps the
/// network's built-in value.
fn override_choices<T: Copy>(
    bardo: &Bardo,
    default: &str,
    all: &[T],
    label: impl Fn(T) -> SharedString,
) -> Choices<Option<T>> {
    let network_default = Choice {
        value: None,
        title: SharedString::from(
            bardo.text_with(Text::RenderPresetNetworkDefault, &[("value", default)]),
        ),
    };
    SearchableVec::new(
        std::iter::once(network_default)
            .chain(all.iter().map(|&value| Choice {
                value: Some(value),
                title: label(value),
            }))
            .collect::<Vec<_>>(),
    )
}

/// Replaces a select's items and keeps its selection when still offered,
/// else selects the first item.
fn set_items<T: Clone + PartialEq + 'static>(
    select: &ChoiceSelect<T>,
    items: Choices<T>,
    window: &mut Window,
    cx: &mut App,
) {
    select.update(cx, |select, cx| {
        let selected = select.selected_value().cloned();
        select.set_items(items, window, cx);
        match selected {
            Some(value) => select.set_selected_value(&value, window, cx),
            None => select.set_selected_index(Some(IndexPath::new(0)), window, cx),
        }
        if select.selected_value().is_none() {
            select.set_selected_index(Some(IndexPath::new(0)), window, cx);
        }
    });
}

/// A select without items; `relabel` fills it for the open form.
fn empty_select<T: Clone + PartialEq + 'static>(
    window: &mut Window,
    cx: &mut App,
) -> ChoiceSelect<T> {
    cx.new(|cx| SelectState::new(SearchableVec::new(Vec::new()), None, window, cx))
}

/// Field errors each input shows, and clears once the user edits it.
const HANDLE_ERRORS: &[NetworkAccountFieldError] = &[
    NetworkAccountFieldError::HandleRequired,
    NetworkAccountFieldError::HandleTooLong,
    NetworkAccountFieldError::HandleInvalid,
];
const TAG_ERRORS: &[NetworkAccountFieldError] = &[
    NetworkAccountFieldError::TooManyTags,
    NetworkAccountFieldError::TagTooLong,
];
const FOOTER_ERRORS: &[NetworkAccountFieldError] =
    &[NetworkAccountFieldError::DescriptionFooterTooLong];
const VISIBILITY_ERRORS: &[NetworkAccountFieldError] =
    &[NetworkAccountFieldError::VisibilityNotOffered];
const BITRATE_ERRORS: &[NetworkAccountFieldError] = &[NetworkAccountFieldError::BitrateInvalid];
const DURATION_ERRORS: &[NetworkAccountFieldError] =
    &[NetworkAccountFieldError::MaxDurationInvalid];
const LOUDNESS_ERRORS: &[NetworkAccountFieldError] = &[NetworkAccountFieldError::LoudnessInvalid];

/// The account the form is open for.
#[derive(Clone, Copy, PartialEq)]
enum Editing {
    New(Network),
    Existing(NetworkAccountId, Network),
}

impl Editing {
    fn network(self) -> Network {
        match self {
            Editing::New(network) | Editing::Existing(_, network) => network,
        }
    }
}

/// What the panel says after the last action.
enum Notice {
    Saved,
    Removed,
    Error(Text),
}

/// The channel whose accounts are shown.
#[derive(Clone, Copy)]
struct ChannelRef {
    id: ChannelId,
    language: ContentLanguage,
}

pub struct NetworkAccountsPanel {
    bardo: Entity<Bardo>,
    /// `None` while the channel form creates a new channel.
    channel: Option<ChannelRef>,
    accounts: Vec<NetworkAccount>,
    load_failed: bool,
    editing: Option<Editing>,
    /// The account waiting for the user to confirm its removal.
    confirm_remove: Option<NetworkAccountId>,
    handle: Entity<InputState>,
    tags: Entity<TextareaState>,
    footer: Entity<TextareaState>,
    language: ChoiceSelect<Option<ContentLanguage>>,
    visibility: ChoiceSelect<Visibility>,
    aspect: ChoiceSelect<Option<AspectRatio>>,
    resolution: ChoiceSelect<Option<Resolution>>,
    codec: ChoiceSelect<Option<VideoCodec>>,
    bitrate: Entity<InputState>,
    max_duration: Entity<InputState>,
    loudness: Entity<InputState>,
    field_errors: Vec<NetworkAccountFieldError>,
    notice: Option<Notice>,
    _subscriptions: Vec<Subscription>,
}

impl NetworkAccountsPanel {
    pub fn new(bardo: Entity<Bardo>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input =
            |window: &mut Window, cx: &mut Context<Self>| cx.new(|cx| InputState::new(window, cx));
        let handle = input(window, cx);
        let bitrate = input(window, cx);
        let max_duration = input(window, cx);
        let loudness = input(window, cx);
        let tags = cx.new(|cx| TextareaState::new(window, cx).auto_grow(2, 8));
        let footer = cx.new(|cx| TextareaState::new(window, cx).auto_grow(2, 8));
        let language: ChoiceSelect<Option<ContentLanguage>> = empty_select(window, cx);
        let visibility: ChoiceSelect<Visibility> = empty_select(window, cx);
        let aspect: ChoiceSelect<Option<AspectRatio>> = empty_select(window, cx);
        let resolution: ChoiceSelect<Option<Resolution>> = empty_select(window, cx);
        let codec: ChoiceSelect<Option<VideoCodec>> = empty_select(window, cx);

        let mut subscriptions = vec![
            cx.subscribe(&handle, |this, _, event, cx| {
                this.edited(HANDLE_ERRORS, event, cx)
            }),
            cx.subscribe(&tags, |this, _, event, cx| {
                this.edited(TAG_ERRORS, event, cx)
            }),
            cx.subscribe(&footer, |this, _, event, cx| {
                this.edited(FOOTER_ERRORS, event, cx)
            }),
            cx.subscribe(&bitrate, |this, _, event, cx| {
                this.edited(BITRATE_ERRORS, event, cx)
            }),
            cx.subscribe(&max_duration, |this, _, event, cx| {
                this.edited(DURATION_ERRORS, event, cx)
            }),
            cx.subscribe(&loudness, |this, _, event, cx| {
                this.edited(LOUDNESS_ERRORS, event, cx)
            }),
            cx.subscribe(
                &visibility,
                |this, _, _: &SelectEvent<Choices<Visibility>>, cx| {
                    this.field_errors
                        .retain(|error| !VISIBILITY_ERRORS.contains(error));
                    this.selected(cx)
                },
            ),
            cx.subscribe(
                &language,
                |this, _, _: &SelectEvent<Choices<Option<ContentLanguage>>>, cx| this.selected(cx),
            ),
            cx.subscribe(
                &aspect,
                |this, _, _: &SelectEvent<Choices<Option<AspectRatio>>>, cx| this.selected(cx),
            ),
            cx.subscribe(
                &resolution,
                |this, _, _: &SelectEvent<Choices<Option<Resolution>>>, cx| this.selected(cx),
            ),
            cx.subscribe(
                &codec,
                |this, _, _: &SelectEvent<Choices<Option<VideoCodec>>>, cx| this.selected(cx),
            ),
        ];
        subscriptions.push(cx.observe_in(&bardo, window, |this, _, window, cx| {
            this.relabel(window, cx)
        }));

        Self {
            bardo,
            channel: None,
            accounts: Vec::new(),
            load_failed: false,
            editing: None,
            confirm_remove: None,
            handle,
            tags,
            footer,
            language,
            visibility,
            aspect,
            resolution,
            codec,
            bitrate,
            max_duration,
            loudness,
            field_errors: Vec::new(),
            notice: None,
            _subscriptions: subscriptions,
        }
    }

    /// Shows the accounts of `channel` (with its content language, which
    /// the language default names), or none while a channel is being
    /// created. Closes any open form.
    pub fn set_channel(
        &mut self,
        channel: Option<(ChannelId, ContentLanguage)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let channel = channel.map(|(id, language)| ChannelRef { id, language });
        let same = self.channel.map(|c| c.id) == channel.map(|c| c.id);
        self.channel = channel;
        if !same {
            self.editing = None;
            self.confirm_remove = None;
            self.field_errors.clear();
            self.notice = None;
        }
        self.relabel(window, cx);
    }

    fn reload(&mut self, cx: &App) {
        let Some(channel) = self.channel else {
            self.accounts.clear();
            self.load_failed = false;
            return;
        };
        match self.bardo.read(cx).network_accounts(channel.id) {
            Ok(accounts) => {
                self.accounts = accounts;
                self.load_failed = false;
            }
            Err(_) => {
                self.accounts.clear();
                self.load_failed = true;
            }
        }
    }

    /// Editing a field hides its now-stale error, and the "saved" notice so
    /// it never describes unsaved edits.
    fn edited(
        &mut self,
        fields: &[NetworkAccountFieldError],
        event: &InputEvent,
        cx: &mut Context<Self>,
    ) {
        if !matches!(event, InputEvent::Change) {
            return;
        }
        self.field_errors.retain(|error| !fields.contains(error));
        if matches!(self.notice, Some(Notice::Saved)) {
            self.notice = None;
        }
        cx.notify();
    }

    /// A select changed: the preset preview follows it.
    fn selected(&mut self, cx: &mut Context<Self>) {
        if matches!(self.notice, Some(Notice::Saved)) {
            self.notice = None;
        }
        cx.notify();
    }

    /// Re-applies every translated label and network default the inputs
    /// hold, after the interface language, the channel or the network of
    /// the form changes. Selections are kept where still offered.
    fn relabel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reload(cx);
        let network = self
            .editing
            .map(Editing::network)
            .unwrap_or(Network::YouTube);
        let channel_language = self.channel.map(|c| c.language).unwrap_or_default();
        let preset = network.render_preset();
        let bardo = self.bardo.read(cx);

        let handle = tr(bardo, Text::AccountHandlePlaceholder);
        let tags = tr(bardo, Text::AccountTagsPlaceholder);
        let footer = tr(bardo, Text::AccountFooterPlaceholder);
        let bitrate = SharedString::from(bardo.localized_decimal(&preset.bitrate.to_string()));
        let max_duration = SharedString::from(preset.max_duration.to_string());
        let loudness = SharedString::from(bardo.localized_decimal(&preset.loudness.to_string()));

        let channel_language_name = bardo.text(Text::ContentLanguageName(channel_language));
        let follow_channel = Choice {
            value: None,
            title: SharedString::from(bardo.text_with(
                Text::AccountLanguageChannel,
                &[("language", &channel_language_name)],
            )),
        };
        let languages = SearchableVec::new(
            std::iter::once(follow_channel)
                .chain(ContentLanguage::ALL.map(|value| Choice {
                    value: Some(value),
                    title: tr(bardo, Text::ContentLanguageName(value)),
                }))
                .collect::<Vec<_>>(),
        );
        let visibilities = SearchableVec::new(
            network
                .visibilities()
                .iter()
                .map(|&value| Choice {
                    value,
                    title: tr(bardo, Text::VisibilityName(value)),
                })
                .collect::<Vec<_>>(),
        );
        let aspects = override_choices(bardo, preset.aspect.code(), &AspectRatio::ALL, |aspect| {
            tr(bardo, Text::AspectRatioName(aspect))
        });
        let resolutions = override_choices(
            bardo,
            &preset.resolution.to_string(),
            &Resolution::ALL,
            |r| SharedString::from(r.to_string()),
        );
        let codecs = override_choices(bardo, preset.codec.name(), &VideoCodec::ALL, |codec| {
            SharedString::from(codec.name())
        });

        self.handle
            .update(cx, |input, cx| input.set_placeholder(handle, window, cx));
        self.tags
            .update(cx, |input, cx| input.set_placeholder(tags, window, cx));
        self.footer
            .update(cx, |input, cx| input.set_placeholder(footer, window, cx));
        self.bitrate
            .update(cx, |input, cx| input.set_placeholder(bitrate, window, cx));
        self.max_duration.update(cx, |input, cx| {
            input.set_placeholder(max_duration, window, cx)
        });
        self.loudness
            .update(cx, |input, cx| input.set_placeholder(loudness, window, cx));
        set_items(&self.language, languages, window, cx);
        set_items(&self.visibility, visibilities, window, cx);
        set_items(&self.aspect, aspects, window, cx);
        set_items(&self.resolution, resolutions, window, cx);
        set_items(&self.codec, codecs, window, cx);
        cx.notify();
    }

    fn fill(&mut self, draft: &NetworkAccountDraft, window: &mut Window, cx: &mut Context<Self>) {
        let set = |input: &Entity<InputState>, value: &str, window: &mut Window, cx: &mut App| {
            let value = value.to_owned();
            input.update(cx, |input, cx| input.set_value(value, window, cx));
        };
        set(&self.handle, &draft.handle, window, cx);
        set(&self.bitrate, &draft.bitrate, window, cx);
        set(&self.max_duration, &draft.max_duration, window, cx);
        set(&self.loudness, &draft.loudness, window, cx);
        let tags = draft.tags.join("\n");
        self.tags
            .update(cx, |input, cx| input.set_value(tags, window, cx));
        let footer = draft.description_footer.clone();
        self.footer
            .update(cx, |input, cx| input.set_value(footer, window, cx));
        self.language.update(cx, |select, cx| {
            select.set_selected_value(&draft.language, window, cx)
        });
        self.visibility.update(cx, |select, cx| {
            select.set_selected_value(&draft.visibility, window, cx)
        });
        self.aspect.update(cx, |select, cx| {
            select.set_selected_value(&draft.aspect, window, cx)
        });
        self.resolution.update(cx, |select, cx| {
            select.set_selected_value(&draft.resolution, window, cx)
        });
        self.codec.update(cx, |select, cx| {
            select.set_selected_value(&draft.codec, window, cx)
        });
    }

    fn draft(&self, cx: &App) -> NetworkAccountDraft {
        let text = |input: &Entity<InputState>| input.read(cx).value().to_string();
        NetworkAccountDraft {
            handle: text(&self.handle),
            language: self.language.read(cx).selected_value().copied().flatten(),
            tags: self
                .tags
                .read(cx)
                .value()
                .split(['\n', ','])
                .map(str::to_owned)
                .collect(),
            description_footer: self.footer.read(cx).value().to_string(),
            visibility: self
                .visibility
                .read(cx)
                .selected_value()
                .copied()
                .unwrap_or_default(),
            aspect: self.aspect.read(cx).selected_value().copied().flatten(),
            resolution: self.resolution.read(cx).selected_value().copied().flatten(),
            codec: self.codec.read(cx).selected_value().copied().flatten(),
            bitrate: text(&self.bitrate),
            max_duration: text(&self.max_duration),
            loudness: text(&self.loudness),
        }
    }

    fn open(&mut self, editing: Editing, window: &mut Window, cx: &mut Context<Self>) {
        let draft = match editing {
            Editing::New(_) => NetworkAccountDraft::default(),
            Editing::Existing(id, _) => {
                let Some(account) = self.accounts.iter().find(|a| a.id == id) else {
                    return;
                };
                NetworkAccountDraft::from(&account.details)
            }
        };
        self.editing = Some(editing);
        self.confirm_remove = None;
        self.field_errors.clear();
        self.notice = None;
        // Items first: the network decides the visibilities and defaults.
        self.relabel(window, cx);
        self.fill(&draft, window, cx);
        cx.notify();
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        self.editing = None;
        self.field_errors.clear();
        self.notice = None;
        cx.notify();
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(channel), Some(editing)) = (self.channel, self.editing) else {
            return;
        };
        let draft = self.draft(cx);
        let bardo = self.bardo.read(cx);
        let result = match editing {
            Editing::New(network) => bardo.add_network_account(channel.id, network, draft),
            Editing::Existing(id, _) => bardo.update_network_account(id, draft),
        };
        match result {
            Ok(saved) => {
                self.reload(cx);
                self.editing = Some(Editing::Existing(saved.id, saved.network));
                self.field_errors.clear();
                // Show the normalized values (handle without @, tags).
                self.fill(&NetworkAccountDraft::from(&saved.details), window, cx);
                self.notice = Some(Notice::Saved);
            }
            Err(error) => self.show_error(&error),
        }
        cx.notify();
    }

    fn remove(&mut self, id: NetworkAccountId, cx: &mut Context<Self>) {
        let result = self.bardo.read(cx).remove_network_account(id);
        self.confirm_remove = None;
        match result {
            Ok(()) => {
                if matches!(self.editing, Some(Editing::Existing(editing, _)) if editing == id) {
                    self.editing = None;
                }
                self.notice = Some(Notice::Removed);
            }
            Err(error) => self.show_error(&error),
        }
        self.reload(cx);
        cx.notify();
    }

    fn show_error(&mut self, error: &NetworkAccountError) {
        self.field_errors = error.field_errors().to_vec();
        self.notice = error.form_message().map(Notice::Error);
    }

    fn render_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        self.accounts
            .iter()
            .map(|account| {
                let id = account.id;
                let network = account.network;
                let open = self.editing == Some(Editing::Existing(id, network));
                let custom = !account.details.overrides().is_empty();
                let preset = if custom {
                    kit::status_with(
                        Tone::Accent,
                        IconName::Settings2,
                        tr(bardo, Text::RenderPresetCustom),
                        cx,
                    )
                } else {
                    kit::status_with(
                        Tone::Neutral,
                        IconName::Check,
                        tr(bardo, Text::RenderPresetDefault),
                        cx,
                    )
                };
                let confirm = (self.confirm_remove == Some(id)).then(|| {
                    v_flex()
                        .gap_2()
                        .pt_2()
                        .child(div().text_sm().child(SharedString::from(bardo.text_with(
                            Text::NetworkAccountRemoveConfirm,
                            &[("network", &bardo.text(Text::NetworkName(network)))],
                        ))))
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    Button::new(("confirm-remove-account", network as usize))
                                        .small()
                                        .danger()
                                        .label(tr(bardo, Text::ConfirmRemoveNetworkAccount))
                                        .on_click(cx.listener(
                                            move |this, _: &ClickEvent, _, cx| this.remove(id, cx),
                                        )),
                                )
                                .child(
                                    Button::new(("keep-account", network as usize))
                                        .small()
                                        .ghost()
                                        .label(tr(bardo, Text::KeepNetworkAccount))
                                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                            this.confirm_remove = None;
                                            cx.notify();
                                        })),
                                ),
                        )
                });
                kit::card(cx)
                    .p_3()
                    .gap_1()
                    .when(open, |card| card.border_color(look(cx).tokens.accent_edge))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                div()
                                    .font_semibold()
                                    .child(tr(bardo, Text::NetworkName(network))),
                            )
                            .child(div().text_sm().text_color(theme.muted_foreground).child(
                                SharedString::from(format!("@{}", account.details.handle())),
                            ))
                            .child(preset)
                            .child(div().flex_1())
                            .child(
                                Button::new(("edit-account", network as usize))
                                    .small()
                                    .ghost()
                                    .label(tr(bardo, Text::EditNetworkAccount))
                                    .on_click(cx.listener(
                                        move |this, _: &ClickEvent, window, cx| {
                                            this.open(Editing::Existing(id, network), window, cx)
                                        },
                                    )),
                            )
                            .child({
                                // Removing is rare: it waits in the "⋯" menu.
                                let panel = cx.entity();
                                let remove = tr(bardo, Text::RemoveNetworkAccount);
                                Button::new(("account-more", network as usize))
                                    .small()
                                    .ghost()
                                    .icon(IconName::Ellipsis)
                                    .dropdown_menu(move |menu, _, _| {
                                        let panel = panel.clone();
                                        menu.item(PopupMenuItem::new(remove.clone()).on_click(
                                            move |_, _, cx| {
                                                panel.update(cx, |this, cx| {
                                                    this.confirm_remove = Some(id);
                                                    this.notice = None;
                                                    cx.notify();
                                                });
                                            },
                                        ))
                                    })
                            }),
                    )
                    .child(div().text_xs().text_color(theme.muted_foreground).child(
                        SharedString::from(bardo.preset_summary(&account.render_preset())),
                    ))
                    .children(confirm)
                    .into_any_element()
            })
            .collect()
    }

    fn render_add(&self, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let taken: Vec<Network> = self.accounts.iter().map(|a| a.network).collect();
        let free: Vec<Network> = Network::ALL
            .into_iter()
            .filter(|network| !taken.contains(network))
            .collect();
        if free.is_empty() {
            return div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(tr(bardo, Text::AllNetworksAdded))
                .into_any_element();
        }
        h_flex()
            .flex_wrap()
            .gap_2()
            .children(free.into_iter().map(|network| {
                Button::new(("add-account", network as usize))
                    .small()
                    .outline()
                    .label(SharedString::from(bardo.text_with(
                        Text::AddNetworkAccount,
                        &[("network", &bardo.text(Text::NetworkName(network)))],
                    )))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.open(Editing::New(network), window, cx)
                    }))
            }))
            .into_any_element()
    }

    fn render_notice(&self, cx: &App) -> Option<AnyElement> {
        let bardo = self.bardo.read(cx);
        let (text, tone) = match self.notice.as_ref()? {
            Notice::Saved => (Text::NetworkAccountSaved, Tone::Success),
            Notice::Removed => (Text::NetworkAccountRemoved, Tone::Success),
            Notice::Error(text) => (*text, Tone::Danger),
        };
        Some(kit::notice(tone, tr(bardo, text), cx).into_any_element())
    }

    fn render_form(&self, editing: Editing, cx: &mut Context<Self>) -> AnyElement {
        let notice = self.render_notice(cx);
        let draft = self.draft(cx);
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let network = editing.network();
        let network_name = bardo.text(Text::NetworkName(network)).into_owned();
        let error_for = |fields: &[NetworkAccountFieldError]| -> Option<AnyElement> {
            let error = self.field_errors.iter().find(|e| fields.contains(e))?;
            Some(
                div()
                    .text_xs()
                    .text_color(theme.danger)
                    .child(tr(bardo, Text::NetworkAccountFieldError(*error)))
                    .into_any_element(),
            )
        };
        let hint = |text: SharedString| {
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(text)
                .into_any_element()
        };
        let field = |label: Text, control: AnyElement, below: Option<AnyElement>| {
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_1()
                .child(div().text_sm().font_medium().child(tr(bardo, label)))
                .child(control)
                .children(below)
        };
        let section = |id: &'static str, title: Text, about: Text| {
            h_flex()
                .pt_2()
                .gap_1()
                .child(div().font_semibold().child(tr(bardo, title)))
                .child(kit::info(id, None, tr(bardo, about)))
        };

        let (title, action) = match editing {
            Editing::New(_) => (Text::NewNetworkAccountTitle, Text::CreateNetworkAccount),
            Editing::Existing(..) => (Text::EditNetworkAccountTitle, Text::SaveNetworkAccount),
        };
        let only_public = network.visibilities().len() == 1;
        let visibility: AnyElement = if only_public {
            hint(SharedString::from(bardo.text_with(
                Text::AccountVisibilityOnlyPublic,
                &[("network", &network_name)],
            )))
        } else {
            Select::new(&self.visibility).into_any_element()
        };
        let preview = SharedString::from(bardo.text_with(
            Text::RenderPresetEffective,
            &[(
                "summary",
                &bardo.preset_summary(&draft.render_preset(network)),
            )],
        ));

        kit::card(cx)
            .p_4()
            .gap_3()
            .border_color(look(cx).tokens.accent_edge)
            .child(div().text_lg().font_semibold().child(SharedString::from(
                bardo.text_with(title, &[("network", &network_name)]),
            )))
            .child(field(
                Text::AccountHandle,
                Input::new(&self.handle).into_any_element(),
                error_for(HANDLE_ERRORS),
            ))
            .child(section(
                "account-metadata-info",
                Text::AccountMetadataTitle,
                Text::AccountMetadataHint,
            ))
            .child(
                h_flex()
                    .gap_4()
                    .items_start()
                    .child(field(
                        Text::AccountLanguage,
                        Select::new(&self.language).into_any_element(),
                        None,
                    ))
                    .child(field(
                        Text::AccountVisibility,
                        visibility,
                        error_for(VISIBILITY_ERRORS),
                    )),
            )
            .child(field(
                Text::AccountTags,
                Textarea::new(&self.tags).into_any_element(),
                error_for(TAG_ERRORS).or_else(|| Some(hint(tr(bardo, Text::AccountTagsHint)))),
            ))
            .child(field(
                Text::AccountFooter,
                Textarea::new(&self.footer).into_any_element(),
                error_for(FOOTER_ERRORS),
            ))
            .child(section(
                "render-preset-info",
                Text::RenderPresetTitle,
                Text::RenderPresetHint,
            ))
            .child(
                h_flex()
                    .gap_4()
                    .items_start()
                    .child(field(
                        Text::RenderPresetAspect,
                        Select::new(&self.aspect).into_any_element(),
                        None,
                    ))
                    .child(field(
                        Text::RenderPresetResolution,
                        Select::new(&self.resolution).into_any_element(),
                        None,
                    ))
                    .child(field(
                        Text::RenderPresetCodec,
                        Select::new(&self.codec).into_any_element(),
                        None,
                    )),
            )
            .child(
                h_flex()
                    .gap_4()
                    .items_start()
                    .child(field(
                        Text::RenderPresetBitrate,
                        Input::new(&self.bitrate).into_any_element(),
                        error_for(BITRATE_ERRORS),
                    ))
                    .child(field(
                        Text::RenderPresetMaxDuration,
                        Input::new(&self.max_duration).into_any_element(),
                        error_for(DURATION_ERRORS)
                            .or_else(|| Some(hint(tr(bardo, Text::RenderPresetMaxDurationHint)))),
                    ))
                    .child(field(
                        Text::RenderPresetLoudness,
                        Input::new(&self.loudness).into_any_element(),
                        error_for(LOUDNESS_ERRORS),
                    )),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(preview),
            )
            .child(
                h_flex()
                    .gap_3()
                    .child(
                        Button::new("save-account")
                            .primary()
                            .label(tr(bardo, action))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.save(window, cx)
                            })),
                    )
                    .child(
                        Button::new("close-account")
                            .ghost()
                            .label(tr(bardo, Text::CancelNetworkAccount))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.close(cx))),
                    )
                    .children(notice),
            )
            .into_any_element()
    }
}

impl Render for NetworkAccountsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let form = self.editing.map(|editing| self.render_form(editing, cx));
        let rows = self.render_rows(cx);
        let add = (self.channel.is_some() && !self.load_failed && self.editing.is_none())
            .then(|| self.render_add(cx));
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let muted = |text: Text| {
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(tr(bardo, text))
                .into_any_element()
        };

        let body: Vec<AnyElement> = if self.channel.is_none() {
            vec![muted(Text::ChannelAccountsSaveFirst)]
        } else if self.load_failed {
            vec![
                kit::notice(Tone::Danger, tr(bardo, Text::ChannelAccountsNotLoaded), cx)
                    .into_any_element(),
            ]
        } else if rows.is_empty() && form.is_none() {
            vec![muted(Text::ChannelAccountsEmpty)]
        } else {
            rows
        };
        // With the form open, the notice sits next to its buttons.
        let notice = self
            .editing
            .is_none()
            .then(|| self.render_notice(cx))
            .flatten();

        v_flex()
            .gap_3()
            .pt_4()
            .border_t_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_xl()
                            .font_semibold()
                            .child(tr(bardo, Text::ChannelAccountsTitle)),
                    )
                    .child(kit::info(
                        "channel-accounts-info",
                        None,
                        tr(bardo, Text::ChannelAccountsHint),
                    )),
            )
            .children(body)
            .children(form)
            .children(add)
            .children(notice)
    }
}
