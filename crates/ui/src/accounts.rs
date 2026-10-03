//! Accounts screen: the channels as a collection, and the picked channel's
//! network accounts as its inspector. The accounts panel holds the
//! behavior; this screen only picks the channel.

use std::rc::Rc;

use bardo_app::bardo_domain::{Channel, ChannelId};
use bardo_app::{Bardo, Destination, Text};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{ClickEvent, Entity, EventEmitter, SharedString, Subscription, Window, div};

use crate::kit::{self, Tone};
use crate::layout;
use crate::network_accounts::{NetworkAccountsPanel, OpenNetworkSettings};
use crate::parts::{Collection, CollectionKind, Header, Inspector, ScreenParts, Tile};
use crate::shell::tr;

pub struct AccountsScreen {
    bardo: Entity<Bardo>,
    channels: Vec<Channel>,
    load_failed: bool,
    channel: Option<ChannelId>,
    panel: Entity<NetworkAccountsPanel>,
    _subscription: Subscription,
}

/// The panel's request to open Settings › Networks goes on to the shell.
impl EventEmitter<OpenNetworkSettings> for AccountsScreen {}

impl AccountsScreen {
    pub fn new(bardo: Entity<Bardo>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let panel = cx.new(|cx| NetworkAccountsPanel::new(bardo.clone(), window, cx));
        let subscription = cx.subscribe(&panel, |_, _, _: &OpenNetworkSettings, cx| {
            cx.emit(OpenNetworkSettings)
        });
        let mut screen = Self {
            bardo,
            channels: Vec::new(),
            load_failed: false,
            channel: None,
            panel,
            _subscription: subscription,
        };
        screen.reload(window, cx);
        screen
    }

    /// Re-reads the channels (made or renamed on the channels screen),
    /// keeping the picked one while it exists.
    pub fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.bardo.read(cx).channels() {
            Ok(channels) => {
                self.channels = channels;
                self.load_failed = false;
            }
            Err(_) => {
                self.channels.clear();
                self.load_failed = true;
            }
        }
        let keep = self
            .channel
            .filter(|id| self.channels.iter().any(|channel| channel.id == *id));
        let pick = keep.or_else(|| self.channels.first().map(|channel| channel.id));
        self.pick(pick, window, cx);
    }

    fn pick(&mut self, id: Option<ChannelId>, window: &mut Window, cx: &mut Context<Self>) {
        self.channel = id;
        let channel = id
            .and_then(|id| self.channels.iter().find(|channel| channel.id == id))
            .map(|channel| (channel.id, channel.details.language()));
        self.panel
            .update(cx, |panel, cx| panel.set_channel(channel, window, cx));
        cx.notify();
    }

    fn collection(&self, cx: &mut Context<Self>) -> Collection {
        let mut collection = Collection::new(CollectionKind::List, "accounts-channels");
        collection.tiles = self
            .channels
            .iter()
            .enumerate()
            .map(|(ix, channel)| {
                let id = channel.id;
                let mut tile = Tile::new(
                    ("accounts-channel", ix),
                    Rc::new(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.pick(Some(id), window, cx)
                    })),
                );
                tile.selected = self.channel == Some(id);
                tile.title = Some(SharedString::from(channel.details.name().to_owned()));
                tile.text = Some(tr(
                    self.bardo.read(cx),
                    Text::ContentLanguageName(channel.details.language()),
                ));
                tile
            })
            .collect();
        let bardo = self.bardo.read(cx);
        collection.empty = Some(if self.load_failed {
            kit::notice(Tone::Danger, tr(bardo, Text::ChannelsNotLoaded), cx).into_any_element()
        } else {
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(tr(bardo, Text::ChannelsEmpty))
                .into_any_element()
        });
        collection
    }
}

impl Render for AccountsScreen {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let collection = self.collection(cx);
        let bardo = self.bardo.read(cx);
        let mut header = Header::place(bardo, Destination::Accounts);
        header.info = Some(
            kit::info("accounts-info", None, tr(bardo, Text::ChannelAccountsHint))
                .into_any_element(),
        );
        let mut parts = ScreenParts::new(header);
        parts.collection = Some(collection);
        parts.inspector = self
            .channel
            .is_some()
            .then(|| Inspector::new(vec![self.panel.clone().into_any_element()]));
        layout::screen(parts, cx)
    }
}
