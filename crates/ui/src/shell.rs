use bardo_app::bardo_domain::UiLanguage;
use bardo_app::{Bardo, Text};
use gpui_kit::component::button::{Button, ButtonGroup};
use gpui_kit::component::{ActiveTheme as _, Selectable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{Entity, SharedString, Window, div, px};

use crate::channels::ChannelsScreen;

/// A UI string in the active language.
pub(crate) fn tr(bardo: &Bardo, text: Text) -> SharedString {
    SharedString::from(bardo.text(text).into_owned())
}

/// The main window: top bar with the interface language switch, and the
/// channels screen below. `Bardo` lives in an entity so screens re-render
/// when it changes (e.g. the language).
pub struct Shell {
    bardo: Entity<Bardo>,
    channels: Entity<ChannelsScreen>,
    error: Option<Text>,
}

impl Shell {
    pub fn new(bardo: Bardo, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let bardo = cx.new(|_| bardo);
        let channels = cx.new(|cx| ChannelsScreen::new(bardo.clone(), window, cx));
        Self {
            bardo,
            channels,
            error: None,
        }
    }

    fn select_language(
        &mut self,
        language: UiLanguage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let result = self.bardo.update(cx, |bardo, cx| {
            let result = bardo.set_ui_language(language);
            cx.notify();
            result
        });
        self.error = result.err().map(|_| Text::LanguageNotSaved);
        window.set_window_title(&self.bardo.read(cx).text(Text::AppName));
        cx.notify();
    }
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bardo = self.bardo.read(cx);
        let current = bardo.ui_language();
        let language_switch = ButtonGroup::new("ui-language")
            .outline()
            .children(UiLanguage::ALL.map(|language| {
                Button::new(language.tag())
                    .label(tr(bardo, Text::LanguageName(language)))
                    .selected(language == current)
            }))
            .on_click(cx.listener(|this, clicked: &Vec<usize>, window, cx| {
                if let Some(language) = clicked.first().and_then(|&i| UiLanguage::ALL.get(i)) {
                    this.select_language(*language, window, cx);
                }
            }));

        let top_bar = h_flex()
            .h(px(48.))
            .px_4()
            .gap_3()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(div().text_lg().child(tr(bardo, Text::AppName)))
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr(bardo, Text::AppTagline)),
            )
            .child(div().flex_1())
            .children(
                self.error
                    .map(|error| div().text_color(cx.theme().danger).child(tr(bardo, error))),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr(bardo, Text::UiLanguageLabel)),
            )
            .child(language_switch);

        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(top_bar)
            .child(div().flex_1().min_h_0().child(self.channels.clone()))
    }
}
