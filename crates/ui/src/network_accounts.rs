//! The network accounts of the channel picked on the accounts screen: one
//! row per account, buttons to add the networks still
//! free, and an inline form for the account being added or edited. Rules
//! and persistence live in `bardo_app`; this file only maps the form to a
//! `NetworkAccountDraft` and errors back to fields.
//!
//! An account on a network Bardo signs in to also shows its connection:
//! connect (consent in the system browser, or a token pasted from the
//! network's developer tools), check, reconnect and disconnect, each run
//! off the UI thread.

use std::collections::HashMap;

use bardo_app::bardo_domain::{
    AspectRatio, ChannelId, ContentLanguage, Network, NetworkAccount, NetworkAccountDraft,
    NetworkAccountFieldError, NetworkAccountId, Resolution, SignInFailureKind, SignInMethod,
    VideoCodec, Visibility,
};
use bardo_app::{
    AccountChoice, Bardo, ConnectionError, ConnectionState, Control, GuideRef, NetworkAccountError,
    Text, TokenConnected, TourAnchor,
};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::searchable_list::{SearchableListItem, SearchableVec};
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IconName, IndexPath, Sizable as _, StyledExt as _, h_flex,
    v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Entity, EventEmitter, ScrollHandle, SharedString, Subscription,
    Task, Window, div,
};

use crate::appearance::look;
use crate::guide;
use crate::icons::Lucide;
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

/// Asks the shell to open Settings › Networks, where a connection's app
/// credentials are saved.
pub struct OpenNetworkSettings;

/// Where the user generates a token to paste, for networks that sign in
/// with one.
fn token_tool(network: Network) -> Option<&'static str> {
    match network {
        Network::InstagramReels => Some("https://developers.facebook.com/tools/explorer/"),
        _ => None,
    }
}

/// The guide section that walks through getting a pasted token.
fn token_steps(network: Network) -> Option<GuideRef> {
    match network {
        Network::InstagramReels => Some(guide::refs::INSTAGRAM_TOKEN),
        _ => None,
    }
}

/// Connection work running off the UI thread for one account.
#[derive(Clone, Copy, PartialEq)]
enum Work {
    Connecting,
    Checking,
    Disconnecting,
}

/// What an account's connection line says after the last action.
struct ConnectionNotice {
    tone: Tone,
    text: SharedString,
    /// Offer to open Settings › Networks: the app credentials are missing
    /// or the network rejected them.
    settings: bool,
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
    /// Connection work in flight; dropping its task stops waiting for it.
    work: HashMap<NetworkAccountId, (Work, Task<()>)>,
    connection_notices: HashMap<NetworkAccountId, ConnectionNotice>,
    /// The pasted token, masked; one paste form is open at a time.
    token: Entity<InputState>,
    /// The account whose paste form is open.
    pasting: Option<NetworkAccountId>,
    token_error: Option<Text>,
    /// The scroll the panel sits in, so the tour brings its cards into
    /// view; the screen hands it to its layout.
    scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<OpenNetworkSettings> for NetworkAccountsPanel {}

impl NetworkAccountsPanel {
    pub fn new(bardo: Entity<Bardo>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input =
            |window: &mut Window, cx: &mut Context<Self>| cx.new(|cx| InputState::new(window, cx));
        let handle = input(window, cx);
        let bitrate = input(window, cx);
        let max_duration = input(window, cx);
        let loudness = input(window, cx);
        // Masked like an API key: the token never shows on screen.
        let token = cx.new(|cx| InputState::new(window, cx).masked(true));
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
        subscriptions.push(cx.subscribe_in(
            &token,
            window,
            |this, _, event, window, cx| match event {
                InputEvent::Change => {
                    if this.token_error.take().is_some() {
                        cx.notify();
                    }
                }
                InputEvent::PressEnter { .. } => {
                    if let Some(id) = this.pasting {
                        this.connect_with_token(id, window, cx);
                    }
                }
                _ => {}
            },
        ));
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
            work: HashMap::new(),
            connection_notices: HashMap::new(),
            token,
            pasting: None,
            token_error: None,
            scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        }
    }

    /// The scroll the panel sits in.
    pub fn scroll(&self) -> ScrollHandle {
        self.scroll.clone()
    }

    /// Whether the channel shown has accounts: the screen offers its tour
    /// only then.
    pub fn has_accounts(&self) -> bool {
        self.channel.is_some() && !self.load_failed && !self.accounts.is_empty()
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
            self.connection_notices.clear();
            self.pasting = None;
            self.token_error = None;
            self.token
                .update(cx, |input, cx| input.set_value("", window, cx));
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

    /// Starts a sign-in the network's way: the consent page in the browser,
    /// or the form for a pasted token.
    fn start_connect(
        &mut self,
        id: NetworkAccountId,
        network: Network,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match network.sign_in_method() {
            Some(SignInMethod::PastedToken) => self.open_paste(id, network, window, cx),
            Some(SignInMethod::Browser) | None => self.connect(id, cx),
        }
    }

    fn open_paste(
        &mut self,
        id: NetworkAccountId,
        network: Network,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let placeholder = tr(
            self.bardo.read(cx),
            Text::ConnectionTokenPlaceholder(network),
        );
        self.token.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.set_placeholder(placeholder, window, cx);
        });
        self.pasting = Some(id);
        self.token_error = None;
        self.connection_notices.remove(&id);
        self.token.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    fn close_paste(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pasting = None;
        self.token_error = None;
        self.token
            .update(cx, |input, cx| input.set_value("", window, cx));
        cx.notify();
    }

    /// Checks the pasted token off the UI thread: trades it for a
    /// long-lived one and finds the accounts it reaches.
    fn connect_with_token(
        &mut self,
        id: NetworkAccountId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.work.contains_key(&id) {
            return;
        }
        let pasted = self.token.read(cx).value();
        let connect = self.bardo.update(cx, |bardo, cx| {
            let connect = bardo.connect_with_token(id, &pasted);
            cx.notify();
            connect
        });
        let connect = match connect {
            Ok(connect) => connect,
            Err(ConnectionError::PastedToken(error)) => {
                self.token_error = Some(Text::PastedTokenError(error));
                return cx.notify();
            }
            Err(error) => {
                self.close_paste(window, cx);
                return self.connection_failed(id, &error, cx);
            }
        };
        self.close_paste(window, cx);
        let bardo = self.bardo.clone();
        let task = cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { connect.run() })
                .await;
            let outcome = bardo.update(cx, |bardo, cx| {
                let outcome = bardo.record_token_connect(result);
                cx.notify();
                outcome
            });
            let _ = this.update(cx, |this, cx| {
                if this
                    .work
                    .get(&id)
                    .is_some_and(|(work, _)| *work == Work::Connecting)
                {
                    this.work.remove(&id);
                }
                match outcome {
                    None => {}
                    Some(Ok(TokenConnected::Connected(_) | TokenConnected::Choose)) => {
                        this.connection_notices.remove(&id);
                    }
                    Some(Err(error)) => this.connection_failed(id, &error, cx),
                }
                cx.notify();
            });
        });
        self.connection_notices.remove(&id);
        self.work.insert(id, (Work::Connecting, task));
        cx.notify();
    }

    /// Connects the account the user picked among those the token reached.
    fn choose_account(&mut self, id: NetworkAccountId, index: usize, cx: &mut Context<Self>) {
        let result = self.bardo.update(cx, |bardo, cx| {
            let result = bardo.choose_account(id, index);
            cx.notify();
            result
        });
        match result {
            Ok(_) => {
                self.connection_notices.remove(&id);
            }
            Err(error) => self.connection_failed(id, &error, cx),
        }
        cx.notify();
    }

    /// Starts a browser sign-in: opens the consent page in the system
    /// browser and waits off the UI thread for the browser to come back.
    fn connect(&mut self, id: NetworkAccountId, cx: &mut Context<Self>) {
        let attempt = self.bardo.update(cx, |bardo, cx| {
            let attempt = bardo.connect(id);
            cx.notify();
            attempt
        });
        let attempt = match attempt {
            Ok(attempt) => attempt,
            Err(error) => return self.connection_failed(id, &error, cx),
        };
        cx.open_url(attempt.consent_url());
        let bardo = self.bardo.clone();
        let task = cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { attempt.run() })
                .await;
            let outcome = bardo.update(cx, |bardo, cx| {
                let outcome = bardo.record_connect(result);
                cx.notify();
                outcome
            });
            let _ = this.update(cx, |this, cx| {
                if this
                    .work
                    .get(&id)
                    .is_some_and(|(work, _)| *work == Work::Connecting)
                {
                    this.work.remove(&id);
                }
                match outcome {
                    // Cancelled or replaced while it ran.
                    None => {}
                    Some(Ok(_)) => {
                        this.connection_notices.remove(&id);
                    }
                    Some(Err(error)) => this.connection_failed(id, &error, cx),
                }
                cx.notify();
            });
        });
        self.connection_notices.remove(&id);
        self.work.insert(id, (Work::Connecting, task));
        cx.notify();
    }

    fn cancel_connect(&mut self, id: NetworkAccountId, cx: &mut Context<Self>) {
        self.bardo.update(cx, |bardo, cx| {
            bardo.cancel_connect(id);
            cx.notify();
        });
        self.work.remove(&id);
        self.connection_notices.remove(&id);
        cx.notify();
    }

    /// Renews the access if due and reads the channel again.
    fn check_connection(&mut self, id: NetworkAccountId, network: Network, cx: &mut Context<Self>) {
        let check = match self.bardo.read(cx).connection_check(id) {
            Ok(check) => check,
            Err(error) => return self.connection_failed(id, &error, cx),
        };
        let bardo = self.bardo.clone();
        let task = cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { check.run() })
                .await;
            // The channel name or the status may have changed.
            bardo.update(cx, |_, cx| cx.notify());
            let _ = this.update(cx, |this, cx| {
                this.work.remove(&id);
                match result {
                    Ok(identity) => {
                        let bardo = this.bardo.read(cx);
                        let text = bardo.text_with(
                            Text::ConnectionChecked(network),
                            &[("channel", &identity.name)],
                        );
                        this.connection_notices.insert(
                            id,
                            ConnectionNotice {
                                tone: Tone::Success,
                                text: SharedString::from(text),
                                settings: false,
                            },
                        );
                    }
                    Err(error) => this.connection_failed(id, &error, cx),
                }
                cx.notify();
            });
        });
        self.connection_notices.remove(&id);
        self.work.insert(id, (Work::Checking, task));
        cx.notify();
    }

    /// Revokes the access and forgets the tokens.
    fn disconnect(&mut self, id: NetworkAccountId, network: Network, cx: &mut Context<Self>) {
        let disconnection = self.bardo.update(cx, |bardo, cx| {
            let disconnection = bardo.disconnection(id);
            cx.notify();
            disconnection
        });
        let disconnection = match disconnection {
            Ok(disconnection) => disconnection,
            Err(error) => return self.connection_failed(id, &error, cx),
        };
        let bardo = self.bardo.clone();
        let task = cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { disconnection.run() })
                .await;
            bardo.update(cx, |_, cx| cx.notify());
            let _ = this.update(cx, |this, cx| {
                this.work.remove(&id);
                match result {
                    Ok(done) => {
                        let bardo = this.bardo.read(cx);
                        let network_name = bardo.text(Text::NetworkName(network));
                        let (tone, text) = if done.revoked {
                            (Tone::Success, bardo.text(Text::ConnectionDisconnected))
                        } else if done.credentials_refused {
                            (
                                Tone::Warning,
                                bardo
                                    .text_with(
                                        Text::ConnectionDisconnectedCredentialsRefused,
                                        &[("network", &network_name)],
                                    )
                                    .into(),
                            )
                        } else if done.kept_for_others {
                            (
                                Tone::Info,
                                bardo
                                    .text_with(
                                        Text::ConnectionDisconnectedKeptForOthers,
                                        &[("network", &network_name)],
                                    )
                                    .into(),
                            )
                        } else {
                            (
                                Tone::Warning,
                                bardo
                                    .text_with(
                                        Text::ConnectionDisconnectedNotRevoked,
                                        &[("network", &network_name)],
                                    )
                                    .into(),
                            )
                        };
                        let text = SharedString::from(text.into_owned());
                        this.connection_notices.insert(
                            id,
                            ConnectionNotice {
                                tone,
                                text,
                                settings: false,
                            },
                        );
                    }
                    Err(error) => this.connection_failed(id, &error, cx),
                }
                cx.notify();
            });
        });
        self.connection_notices.remove(&id);
        self.work.insert(id, (Work::Disconnecting, task));
        cx.notify();
    }

    fn connection_failed(&mut self, id: NetworkAccountId, error: &ConnectionError, cx: &App) {
        let Some(text) = error.message() else {
            return;
        };
        let tone = match error {
            ConnectionError::Consent(bardo_app::bardo_domain::ConsentError::Cancelled) => {
                self.connection_notices.remove(&id);
                return;
            }
            ConnectionError::ReconnectNeeded(_) => Tone::Warning,
            ConnectionError::SignIn(_, failure)
                if matches!(
                    failure.kind,
                    SignInFailureKind::LimitReached
                        | SignInFailureKind::NetworkDown
                        | SignInFailureKind::Unreachable
                ) =>
            {
                Tone::Warning
            }
            _ => Tone::Danger,
        };
        let settings = matches!(error, ConnectionError::NoAppCredentials(_))
            || matches!(
                error,
                ConnectionError::SignIn(_, failure) if failure.kind == SignInFailureKind::ClientRejected
            );
        self.connection_notices.insert(
            id,
            ConnectionNotice {
                tone,
                text: tr(self.bardo.read(cx), text),
                settings,
            },
        );
    }

    /// The connection line of an account on a network Bardo signs in to.
    fn render_connection(
        &self,
        account: &NetworkAccount,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let bardo = self.bardo.read(cx);
        let id = account.id;
        let network = account.network;
        let state = match bardo.connection_state(account) {
            Ok(ConnectionState::Unavailable) => return None,
            Ok(state) => state,
            Err(error) => {
                let text = error.message().unwrap_or(Text::ConnectionNotSaved);
                return Some(kit::notice(Tone::Danger, tr(bardo, text), cx).into_any_element());
            }
        };
        let work = self.work.get(&id).map(|(work, _)| *work);
        let ix = network as usize;
        let small =
            |id: &'static str, text: Text| Button::new((id, ix)).small().label(tr(bardo, text));
        let pill = match &state {
            ConnectionState::Unavailable | ConnectionState::NotConnected => kit::status_with(
                Tone::Neutral,
                IconName::Minus,
                tr(bardo, Text::ConnectionNotConnectedLabel),
                cx,
            ),
            ConnectionState::Connecting => kit::status_with(
                Tone::Info,
                IconName::LoaderCircle,
                tr(bardo, Text::ConnectionConnecting(network)),
                cx,
            ),
            ConnectionState::Choosing { .. } => kit::status_with(
                Tone::Info,
                IconName::ChevronsUpDown,
                tr(bardo, Text::ConnectionChooseLabel),
                cx,
            ),
            ConnectionState::Connected { channel } => kit::status(
                Tone::Success,
                bardo.text_with(Text::ConnectionConnected, &[("channel", channel)]),
                cx,
            ),
            ConnectionState::ReconnectNeeded { channel } => kit::status(
                Tone::Warning,
                bardo.text_with(Text::ConnectionReconnectNeeded, &[("channel", channel)]),
                cx,
            ),
        };
        let disconnect = || {
            small("disconnect-account", Text::Disconnect)
                .ghost()
                .loading(work == Some(Work::Disconnecting))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    if !this.work.contains_key(&id) {
                        this.disconnect(id, network, cx)
                    }
                }))
        };
        let actions: Vec<AnyElement> = match &state {
            ConnectionState::Unavailable => Vec::new(),
            ConnectionState::NotConnected if self.pasting == Some(id) => Vec::new(),
            ConnectionState::NotConnected => vec![
                small("connect-account", Text::Connect)
                    .primary()
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.start_connect(id, network, window, cx)
                    }))
                    .into_any_element(),
            ],
            ConnectionState::Connecting | ConnectionState::Choosing { .. } => vec![
                small("cancel-connect", Text::CancelConnect)
                    .ghost()
                    .on_click(
                        cx.listener(move |this, _: &ClickEvent, _, cx| this.cancel_connect(id, cx)),
                    )
                    .into_any_element(),
            ],
            ConnectionState::Connected { .. } => vec![
                small(
                    "check-connection",
                    if work == Some(Work::Checking) {
                        Text::CheckingConnection
                    } else {
                        Text::CheckConnection
                    },
                )
                .outline()
                .loading(work == Some(Work::Checking))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    if !this.work.contains_key(&id) {
                        this.check_connection(id, network, cx)
                    }
                }))
                .into_any_element(),
                disconnect().into_any_element(),
            ],
            ConnectionState::ReconnectNeeded { .. } if self.pasting == Some(id) => {
                vec![disconnect().into_any_element()]
            }
            ConnectionState::ReconnectNeeded { .. } => vec![
                small("reconnect-account", Text::Reconnect)
                    .primary()
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.start_connect(id, network, window, cx)
                    }))
                    .into_any_element(),
                disconnect().into_any_element(),
            ],
        };
        let choices = match &state {
            ConnectionState::Choosing { choices } => Some(self.render_choices(id, choices, cx)),
            _ => None,
        };
        let paste = (self.pasting == Some(id)
            && matches!(
                state,
                ConnectionState::NotConnected | ConnectionState::ReconnectNeeded { .. }
            ))
        .then(|| self.render_paste(id, network, cx));
        let hint = matches!(state, ConnectionState::ReconnectNeeded { .. }).then(|| {
            div()
                .text_xs()
                .text_color(look(cx).tokens.text2)
                .child(tr(bardo, Text::ConnectionReconnectHint(network)))
        });
        let notice =
            self.connection_notices.get(&id).map(|notice| {
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .child(kit::notice(notice.tone, notice.text.clone(), cx))
                    .when(notice.settings, |row| {
                        row.child(
                            Button::new(("open-network-settings", ix))
                                .small()
                                .outline()
                                .icon(IconName::Settings2)
                                .label(tr(bardo, Text::OpenNetworkSettings))
                                .on_click(cx.listener(|_, _: &ClickEvent, _, cx| {
                                    cx.emit(OpenNetworkSettings)
                                })),
                        )
                    })
            });
        Some(
            v_flex()
                .gap_1p5()
                .pt_1()
                .child(
                    h_flex()
                        .flex_wrap()
                        .items_center()
                        .gap_2()
                        .child(pill)
                        .child(div().flex_1())
                        .children(actions),
                )
                .children(hint)
                .children(paste)
                .children(choices)
                .children(notice)
                .into_any_element(),
        )
    }

    /// The form for a token pasted from the network's developer tools.
    fn render_paste(
        &self,
        id: NetworkAccountId,
        network: Network,
        cx: &Context<Self>,
    ) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let tokens = look(cx).tokens;
        let ix = network as usize;
        let error = self.token_error.map(|error| {
            div()
                .text_xs()
                .text_color(tokens.danger)
                .child(tr(bardo, error))
        });
        let working = self.work.contains_key(&id);
        kit::well(cx)
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .child(
                div()
                    .text_sm()
                    .font_medium()
                    .child(tr(bardo, Text::ConnectionTokenLabel(network))),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(tokens.text2)
                    .child(tr(bardo, Text::ConnectionTokenHelp(network))),
            )
            .child(Input::new(&self.token))
            .children(error)
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        Button::new(("connect-with-token", ix))
                            .small()
                            .primary()
                            .disabled(working)
                            .label(tr(bardo, Text::Connect))
                            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                this.connect_with_token(id, window, cx)
                            })),
                    )
                    .children(token_tool(network).map(|url| {
                        Button::new(("open-token-tool", ix))
                            .small()
                            .outline()
                            .icon(IconName::ExternalLink)
                            .label(tr(bardo, Text::ConnectionOpenTokenTool(network)))
                            .on_click(move |_, _, cx| cx.open_url(url))
                    }))
                    .children(token_steps(network).map(|steps| {
                        Button::new(("token-steps", ix))
                            .small()
                            .ghost()
                            .icon(Lucide::BookOpen)
                            .label(tr(bardo, Text::SetupStepByStep))
                            .on_click(move |_, window, cx| guide::open_section(steps, window, cx))
                    }))
                    .child(div().flex_1())
                    .child(
                        Button::new(("cancel-paste", ix))
                            .small()
                            .ghost()
                            .label(tr(bardo, Text::CancelConnect))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.close_paste(window, cx)
                            })),
                    ),
            )
            .into_any_element()
    }

    /// The accounts a pasted token reached, one row each.
    fn render_choices(
        &self,
        id: NetworkAccountId,
        choices: &[AccountChoice],
        cx: &Context<Self>,
    ) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let tokens = look(cx).tokens;
        let network = self
            .accounts
            .iter()
            .find(|account| account.id == id)
            .map_or(Network::InstagramReels, |account| account.network);
        let rows =
            choices.iter().enumerate().map(|(index, choice)| {
                h_flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .py_1p5()
                    .px_2()
                    .bg(tokens.sunken)
                    .border(tokens.border_width)
                    .border_color(tokens.border)
                    .rounded(tokens.radius)
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_sm()
                                    .font_medium()
                                    .child(SharedString::from(choice.name.clone())),
                            )
                            .child(div().text_xs().text_color(tokens.text2).child(
                                SharedString::from(bardo.text_with(
                                    Text::ConnectionChoiceVia(network),
                                    &[("via", &choice.via)],
                                )),
                            )),
                    )
                    .child(
                        Button::new(SharedString::from(format!("use-account-{index}")))
                            .small()
                            .outline()
                            .label(tr(bardo, Text::ConnectionUseAccount))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.choose_account(id, index, cx)
                            })),
                    )
                    .into_any_element()
            });
        v_flex()
            .gap_1p5()
            .child(
                div()
                    .text_sm()
                    .child(tr(bardo, Text::ConnectionChoose(network))),
            )
            .children(rows.collect::<Vec<_>>())
            .into_any_element()
    }

    fn show_error(&mut self, error: &NetworkAccountError) {
        self.field_errors = error.field_errors().to_vec();
        self.notice = error.form_message().map(Notice::Error);
    }

    fn render_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut connections: HashMap<NetworkAccountId, AnyElement> = self
            .accounts
            .iter()
            .filter_map(|account| Some((account.id, self.render_connection(account, cx)?)))
            .collect();
        // The tour points at the first card, and at the first connection.
        let first_connected = self
            .accounts
            .iter()
            .find(|account| connections.contains_key(&account.id))
            .map(|account| account.id);
        let scroll = Some(&self.scroll);
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        self.accounts
            .iter()
            .enumerate()
            .map(|(index, account)| {
                let first = index == 0;
                let tagged = |anchor: TourAnchor, element: AnyElement| {
                    if first {
                        kit::anchor_in(anchor, element, scroll).into_any_element()
                    } else {
                        element
                    }
                };
                let id = account.id;
                let network = account.network;
                let open = self.editing == Some(Editing::Existing(id, network));
                let custom = !account.details.overrides().is_empty();
                let connection = connections.remove(&id).map(|connection| {
                    if first_connected == Some(id) {
                        kit::anchor_in(
                            TourAnchor::Control(Control::AccountConnection),
                            connection,
                            scroll,
                        )
                        .into_any_element()
                    } else {
                        connection
                    }
                });
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
                let card = kit::card(cx)
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
                            .child(tagged(
                                TourAnchor::Control(Control::AccountEdit),
                                Button::new(("edit-account", network as usize))
                                    .small()
                                    .ghost()
                                    .label(tr(bardo, Text::EditNetworkAccount))
                                    .on_click(cx.listener(
                                        move |this, _: &ClickEvent, window, cx| {
                                            this.open(Editing::Existing(id, network), window, cx)
                                        },
                                    ))
                                    .into_any_element(),
                            ))
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
                    .child(tagged(
                        TourAnchor::Control(Control::AccountPreset),
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(SharedString::from(
                                bardo.preset_summary(&account.render_preset()),
                            ))
                            .into_any_element(),
                    ))
                    .children(connection)
                    .children(confirm)
                    .into_any_element();
                tagged(TourAnchor::Control(Control::AccountCard), card)
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
        let buttons = h_flex()
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
            }));
        kit::anchor_in(
            TourAnchor::Control(Control::AccountAdd),
            buttons,
            Some(&self.scroll),
        )
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
        let section = |id: &'static str, title: Text, about: Text, more: GuideRef| {
            h_flex()
                .pt_2()
                .gap_1()
                .child(div().font_semibold().child(tr(bardo, title)))
                .child(guide::info(bardo, id, tr(bardo, about), more))
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
                guide::refs::ACCOUNTS_METADATA,
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
                guide::refs::ACCOUNTS_PRESET,
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
            .child(kit::section_heading(tr(bardo, Text::ChannelAccountsTitle)))
            .children(body)
            .children(form)
            .children(add)
            .children(notice)
    }
}
