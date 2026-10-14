//! Privacy and YouTube (issue #145, ADR-0012): the bar over the screens
//! while YouTube waits for the privacy policy, the consent block on the
//! YouTube cards, the links Bardo shows beside YouTube, and the Privacy and
//! YouTube section of Settings › About. What is kept, refused and deleted
//! lives in `bardo_app`; the daily upkeep runs off the UI thread.

use std::time::Duration;

use bardo_app::bardo_domain::UPKEEP_EVERY;
use bardo_app::{
    Bardo, Control, GOOGLE_PERMISSIONS_URL, GOOGLE_PRIVACY_URL, GuidePlace, SettingsTab, Text,
    TourAnchor, YOUTUBE_TERMS_URL, privacy_policy_url,
};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Icon, IconName, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, ElementId, Entity, ScrollHandle, Subscription, Task, WeakEntity,
    Window, div, px,
};

use crate::appearance::look;
use crate::guide;
use crate::kit::{self, Tone};
use crate::shell::tr;

/// How long after Bardo opens its first upkeep waits, so it never competes
/// with the start.
const FIRST_UPKEEP_AFTER: Duration = Duration::from_secs(20);

/// Runs the YouTube upkeep shortly after Bardo opens, then every day, off
/// the UI thread, for as long as the task is kept.
pub fn upkeep_daily(bardo: WeakEntity<Bardo>, cx: &mut App) -> Task<()> {
    cx.spawn(async move |cx| {
        let mut wait = FIRST_UPKEEP_AFTER;
        loop {
            cx.background_executor().timer(wait).await;
            let Some(bardo) = bardo.upgrade() else {
                break;
            };
            let upkeep = cx.update(|cx| bardo.read(cx).youtube_upkeep());
            cx.background_executor()
                .spawn(async move { upkeep.run() })
                .await;
            // A lapsed connection is gone: the cards show it.
            cx.update(|cx| bardo.update(cx, |_, cx| cx.notify()));
            wait = UPKEEP_EVERY;
        }
    })
}

fn accept(bardo: &Entity<Bardo>, cx: &mut App) {
    bardo.update(cx, |bardo, cx| {
        if let Err(error) = bardo.accept_privacy_policy() {
            tracing::warn!("could not save the privacy policy's acceptance: {error}");
        }
        cx.notify();
    });
}

fn read_policy(window: &mut Window, cx: &mut App) {
    guide::open_section(guide::refs::PRIVACY_POLICY, window, cx);
}

/// The bar over the screens while YouTube waits for the privacy policy.
pub fn banner(bardo: &Entity<Bardo>, cx: &App) -> AnyElement {
    let t = look(cx).tokens;
    let read = bardo.read(cx);
    h_flex()
        .id("privacy-banner")
        .w_full()
        .flex_none()
        .flex_wrap()
        .items_center()
        .gap_x_3()
        .gap_y_1p5()
        .px_4()
        .py_1p5()
        .bg(t.selected)
        .border_b(t.border_width)
        .border_color(t.accent_edge)
        .child(
            h_flex()
                .flex_1()
                .min_w(px(200.))
                .gap_2()
                .items_center()
                .child(
                    Icon::new(IconName::Info)
                        .size(px(16.))
                        .text_color(t.accent_text)
                        .flex_none(),
                )
                .child(
                    div()
                        .min_w_0()
                        .text_sm()
                        .font_medium()
                        .text_color(t.text)
                        .child(tr(read, Text::PrivacyBanner)),
                ),
        )
        .child(
            Button::new("privacy-banner-review")
                .small()
                .primary()
                .label(tr(read, Text::PrivacyBannerReview))
                .on_click(|_, window, cx| {
                    guide::go(GuidePlace::Settings(SettingsTab::About), window, cx)
                }),
        )
        .into_any_element()
}

/// "Uses YouTube API Services", with links to YouTube's Terms of Service,
/// Google's Privacy Policy and Bardo's own (policy III.A).
pub fn links(bardo: &Bardo, id: &'static str, cx: &App) -> AnyElement {
    let t = look(cx).tokens;
    let policy = privacy_policy_url(bardo.ui_language());
    let link = |n: usize, text: Text, url: String| {
        Button::new(ElementId::Name(format!("{id}-link-{n}").into()))
            .link()
            .xsmall()
            .label(tr(bardo, text))
            .on_click(move |_, _, cx| cx.open_url(&url))
    };
    h_flex()
        .flex_wrap()
        .items_center()
        .gap_x_3()
        .gap_y_1()
        .child(
            div()
                .text_xs()
                .text_color(t.text2)
                .child(tr(bardo, Text::PrivacyUsesYouTube)),
        )
        .child(link(0, Text::YouTubeTerms, YOUTUBE_TERMS_URL.to_owned()))
        .child(link(
            1,
            Text::GooglePrivacyPolicy,
            GOOGLE_PRIVACY_URL.to_owned(),
        ))
        .child(link(2, Text::PrivacyOnline, policy))
        .into_any_element()
}

/// Asks for the privacy policy where YouTube is set up: why, a way to read
/// it, and Accept. Shown only while it is not accepted.
pub fn consent(bardo: &Entity<Bardo>, id: &'static str, cx: &App) -> AnyElement {
    let t = look(cx).tokens;
    let read = bardo.read(cx);
    let accepting = bardo.clone();
    v_flex()
        .gap_2()
        .p_3()
        .rounded(t.radius)
        .border(t.border_width)
        .border_color(t.accent_edge)
        .bg(t.selected)
        .child(
            h_flex()
                .gap_1p5()
                .items_start()
                .child(
                    div().flex_none().pt(px(2.)).child(
                        Icon::new(IconName::Info)
                            .size(px(14.))
                            .text_color(t.accent_text),
                    ),
                )
                .child(
                    div()
                        .min_w_0()
                        .text_sm()
                        .text_color(t.text)
                        .child(tr(read, Text::PrivacyConsent)),
                ),
        )
        .child(
            h_flex()
                .flex_wrap()
                .gap_2()
                .child(
                    Button::new(ElementId::Name(format!("{id}-privacy-read").into()))
                        .small()
                        .outline()
                        .label(tr(read, Text::PrivacyRead))
                        .on_click(|_, window, cx| read_policy(window, cx)),
                )
                .child(
                    Button::new(ElementId::Name(format!("{id}-privacy-accept").into()))
                        .small()
                        .primary()
                        .label(tr(read, Text::PrivacyAccept))
                        .on_click(move |_, _, cx| accept(&accepting, cx)),
                ),
        )
        .child(links(read, id, cx))
        .into_any_element()
}

/// Settings › About's Privacy and YouTube section.
pub struct PrivacyPanel {
    bardo: Entity<Bardo>,
    /// The settings screen's scroll, so the tour brings it into view.
    scroll: ScrollHandle,
    /// "Delete YouTube data…" is asking first.
    confirming: bool,
    /// What the last deletion did.
    note: Option<(Tone, Text)>,
    _subscriptions: Vec<Subscription>,
}

impl PrivacyPanel {
    pub fn new(bardo: Entity<Bardo>, scroll: ScrollHandle, cx: &mut Context<Self>) -> Self {
        // The acceptance, the language.
        let subscriptions = vec![cx.observe(&bardo, |_, _, cx| cx.notify())];
        Self {
            bardo,
            scroll,
            confirming: false,
            note: None,
            _subscriptions: subscriptions,
        }
    }

    fn delete(&mut self, cx: &mut Context<Self>) {
        self.confirming = false;
        self.note = Some(match self.bardo.read(cx).forget_youtube_data() {
            Ok(_) => (Tone::Success, Text::YouTubeDataDeleted),
            Err(error) => {
                tracing::warn!("could not delete the YouTube data: {error}");
                (Tone::Danger, Text::YouTubeDataNotDeleted)
            }
        });
        // Research, rankings and numbers on other screens are gone.
        self.bardo.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    fn render_delete(&self, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        if !self.confirming {
            return h_flex()
                .flex_wrap()
                .gap_2()
                .child(
                    Button::new("privacy-delete")
                        .small()
                        .outline()
                        .label(tr(bardo, Text::DeleteYouTubeData))
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.confirming = true;
                            this.note = None;
                            cx.notify();
                        })),
                )
                .children(
                    self.note
                        .map(|(tone, text)| kit::notice(tone, tr(bardo, text), cx)),
                )
                .into_any_element();
        }
        kit::card(cx)
            .p_3()
            .gap_2()
            .child(kit::notice(
                Tone::Warning,
                tr(bardo, Text::DeleteYouTubeDataConfirm),
                cx,
            ))
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        Button::new("privacy-delete-now")
                            .small()
                            .danger()
                            .label(tr(bardo, Text::DeleteYouTubeDataNow))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.delete(cx))),
                    )
                    .child(
                        Button::new("privacy-delete-cancel")
                            .small()
                            .ghost()
                            .label(tr(bardo, Text::DeleteYouTubeDataCancel))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.confirming = false;
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }
}

impl Render for PrivacyPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let delete = self.render_delete(cx);
        let bardo = self.bardo.read(cx);
        let t = look(cx).tokens;
        let accepted = bardo.privacy_accepted();
        let state = match bardo.privacy_consent() {
            Some(consent) if accepted => kit::status(
                Tone::Success,
                bardo.text_with(
                    Text::PrivacyAcceptedOn,
                    &[("when", &bardo.backup_time_text(consent.accepted_at))],
                ),
                cx,
            ),
            Some(_) => kit::notice(Tone::Warning, tr(bardo, Text::PrivacyChanged), cx),
            None => kit::notice(Tone::Neutral, tr(bardo, Text::PrivacyNotYet), cx),
        };
        let policy = privacy_policy_url(bardo.ui_language());
        let link = |id: &'static str, text: Text, url: &'static str| {
            Button::new(id)
                .link()
                .xsmall()
                .label(tr(bardo, text))
                .on_click(move |_, _, cx| cx.open_url(url))
        };
        let accepting = self.bardo.clone();
        kit::anchor_in(
            TourAnchor::Control(Control::AboutPrivacy),
            v_flex()
                .gap_2()
                .child(kit::section_heading(tr(bardo, Text::PrivacyTitle)))
                .child(
                    div()
                        .text_sm()
                        .text_color(t.text2)
                        .child(tr(bardo, Text::PrivacySummary)),
                )
                .child(h_flex().child(state))
                .child(
                    h_flex()
                        .flex_wrap()
                        .gap_2()
                        .when(!accepted, |row| {
                            row.child(
                                Button::new("privacy-accept")
                                    .small()
                                    .primary()
                                    .label(tr(bardo, Text::PrivacyAccept))
                                    .on_click(move |_, _, cx| accept(&accepting, cx)),
                            )
                        })
                        .child(
                            Button::new("privacy-read")
                                .small()
                                .label(tr(bardo, Text::PrivacyRead))
                                .on_click(|_, window, cx| read_policy(window, cx)),
                        )
                        .child(
                            Button::new("privacy-online")
                                .small()
                                .label(tr(bardo, Text::PrivacyOnline))
                                .on_click(move |_, _, cx| cx.open_url(&policy)),
                        ),
                )
                .child(
                    h_flex()
                        .flex_wrap()
                        .items_center()
                        .gap_x_3()
                        .gap_y_1()
                        .child(
                            div()
                                .text_xs()
                                .text_color(t.text2)
                                .child(tr(bardo, Text::PrivacyUsesYouTube)),
                        )
                        .child(link("privacy-terms", Text::YouTubeTerms, YOUTUBE_TERMS_URL))
                        .child(link(
                            "privacy-google",
                            Text::GooglePrivacyPolicy,
                            GOOGLE_PRIVACY_URL,
                        ))
                        .child(link(
                            "privacy-permissions",
                            Text::GooglePermissions,
                            GOOGLE_PERMISSIONS_URL,
                        )),
                )
                .child(delete),
            Some(&self.scroll),
        )
    }
}
