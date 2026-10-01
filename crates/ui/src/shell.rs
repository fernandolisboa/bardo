use bardo_app::bardo_domain::UiLanguage;
use bardo_app::{Bardo, Text};
use gpui_kit::component::button::{Button, ButtonGroup};
use gpui_kit::component::{ActiveTheme as _, Selectable as _, h_flex, v_flex};
use gpui_kit::{
    Context, IntoElement, ParentElement as _, Render, SharedString, Styled as _, Window, div,
};

/// The main window. Holds no state of its own beyond what `Bardo` exposes;
/// every string comes from the active language's resource file.
pub struct Shell {
    bardo: Bardo,
    error: Option<Text>,
}

impl Shell {
    pub fn new(bardo: Bardo) -> Self {
        Self { bardo, error: None }
    }

    fn text(&self, text: Text) -> SharedString {
        SharedString::from(self.bardo.text(text).to_owned())
    }

    fn select_language(
        &mut self,
        language: UiLanguage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.error = match self.bardo.set_ui_language(language) {
            Ok(()) => None,
            Err(_) => Some(Text::LanguageNotSaved),
        };
        window.set_window_title(self.bardo.text(Text::AppName));
        cx.notify();
    }
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.bardo.ui_language();
        let language_switch = ButtonGroup::new("ui-language")
            .outline()
            .children(UiLanguage::ALL.map(|language| {
                Button::new(language.tag())
                    .label(self.text(Text::LanguageName(language)))
                    .selected(language == current)
            }))
            .on_click(cx.listener(|this, clicked: &Vec<usize>, window, cx| {
                if let Some(language) = clicked.first().and_then(|&i| UiLanguage::ALL.get(i)) {
                    this.select_language(*language, window, cx);
                }
            }));

        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .items_center()
            .justify_center()
            .gap_6()
            .child(
                v_flex()
                    .items_center()
                    .gap_2()
                    .child(div().text_3xl().child(self.text(Text::AppName)))
                    .child(
                        div()
                            .text_color(cx.theme().muted_foreground)
                            .child(self.text(Text::AppTagline)),
                    ),
            )
            .child(
                h_flex()
                    .gap_3()
                    .child(self.text(Text::UiLanguageLabel))
                    .child(language_switch),
            )
            .children(
                self.error
                    .map(|error| div().text_color(cx.theme().danger).child(self.text(error))),
            )
    }
}
