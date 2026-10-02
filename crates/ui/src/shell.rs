use bardo_app::bardo_domain::{UiLanguage, VideoProjectId};
use bardo_app::{Bardo, Text};
use gpui_kit::component::badge::Badge;
use gpui_kit::component::button::{Button, ButtonGroup, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Selectable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{ClickEvent, Entity, SharedString, Subscription, Window, div, px};

use crate::channels::ChannelsScreen;
use crate::costs::CostsScreen;
use crate::editor::{EditorEvent, EditorScreen};
use crate::jobs::JobsPanel;
use crate::personas::PersonasScreen;
use crate::projects::{OpenEditor, ProjectsScreen};
use crate::research::ResearchScreen;
use crate::settings::SettingsScreen;
use crate::templates::TemplatesScreen;
use crate::themes::ThemesScreen;

/// A UI string in the active language.
pub(crate) fn tr(bardo: &Bardo, text: Text) -> SharedString {
    SharedString::from(bardo.text(text).into_owned())
}

/// The screens the top bar switches between.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Channels,
    Personas,
    Research,
    Themes,
    Projects,
    Templates,
    Costs,
    Settings,
}

impl Screen {
    const ALL: [Screen; 8] = [
        Screen::Channels,
        Screen::Personas,
        Screen::Research,
        Screen::Themes,
        Screen::Projects,
        Screen::Templates,
        Screen::Costs,
        Screen::Settings,
    ];

    fn title(self) -> Text {
        match self {
            Screen::Channels => Text::ChannelsTitle,
            Screen::Personas => Text::PersonasTitle,
            Screen::Research => Text::ResearchTitle,
            Screen::Themes => Text::ThemesTitle,
            Screen::Projects => Text::ProjectsNav,
            Screen::Templates => Text::TemplatesTitle,
            Screen::Costs => Text::CostsTitle,
            Screen::Settings => Text::SettingsTitle,
        }
    }
}

/// The main window: top bar with the screen switch, the jobs toggle and the
/// interface language switch, the current screen below and the jobs panel
/// on the right when open. `Bardo` lives in an entity so screens re-render
/// when it changes (e.g. the language).
pub struct Shell {
    bardo: Entity<Bardo>,
    screen: Screen,
    channels: Entity<ChannelsScreen>,
    personas: Entity<PersonasScreen>,
    research: Entity<ResearchScreen>,
    themes: Entity<ThemesScreen>,
    projects: Entity<ProjectsScreen>,
    templates: Entity<TemplatesScreen>,
    costs: Entity<CostsScreen>,
    settings: Entity<SettingsScreen>,
    /// Kept alive while closed, so the toggle's count stays current.
    jobs: Entity<JobsPanel>,
    jobs_open: bool,
    /// The editor, open over the whole window in place of the screens.
    editor: Option<(Entity<EditorScreen>, Subscription)>,
    error: Option<Text>,
    _subscriptions: Vec<Subscription>,
}

impl Shell {
    pub fn new(bardo: Bardo, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let bardo = cx.new(|_| bardo);
        let channels = cx.new(|cx| ChannelsScreen::new(bardo.clone(), window, cx));
        let personas = cx.new(|cx| PersonasScreen::new(bardo.clone(), window, cx));
        let research = cx.new(|cx| ResearchScreen::new(bardo.clone(), window, cx));
        let themes = cx.new(|cx| ThemesScreen::new(bardo.clone(), window, cx));
        let projects = cx.new(|cx| ProjectsScreen::new(bardo.clone(), window, cx));
        let templates = cx.new(|cx| TemplatesScreen::new(bardo.clone(), window, cx));
        let costs = cx.new(|cx| CostsScreen::new(bardo.clone(), window, cx));
        let settings = cx.new(|cx| SettingsScreen::new(bardo.clone(), window, cx));
        let jobs = cx.new(|cx| JobsPanel::new(bardo.clone(), cx));
        let subscriptions = vec![
            cx.observe(&jobs, |_, _, cx| cx.notify()),
            cx.subscribe_in(
                &projects,
                window,
                |this, _, event: &OpenEditor, window, cx| {
                    this.open_editor(event.0, window, cx);
                },
            ),
        ];
        Self {
            bardo,
            screen: Screen::Channels,
            channels,
            personas,
            research,
            themes,
            projects,
            templates,
            costs,
            settings,
            jobs,
            jobs_open: false,
            editor: None,
            error: None,
            _subscriptions: subscriptions,
        }
    }

    fn show(&mut self, screen: Screen, window: &mut Window, cx: &mut Context<Self>) {
        // Channels may have changed on the channels screen, personas on the
        // personas screen, niches on the research screen, and projects on
        // the themes screen.
        if screen != self.screen {
            match screen {
                Screen::Channels => self
                    .channels
                    .update(cx, |channels, cx| channels.reload_personas(window, cx)),
                Screen::Personas => self
                    .personas
                    .update(cx, |personas, cx| personas.reload(window, cx)),
                Screen::Research => self
                    .research
                    .update(cx, |research, cx| research.reload_channels(window, cx)),
                Screen::Themes => self
                    .themes
                    .update(cx, |themes, cx| themes.reload_channels(window, cx)),
                Screen::Projects => self
                    .projects
                    .update(cx, |projects, cx| projects.reload(window, cx)),
                Screen::Templates => self
                    .templates
                    .update(cx, |templates, cx| templates.reload(window, cx)),
                Screen::Costs => self.costs.update(cx, |costs, cx| costs.reload(cx)),
                Screen::Settings => {}
            }
        }
        self.screen = screen;
        cx.notify();
    }

    fn open_editor(
        &mut self,
        project: VideoProjectId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let bardo = self.bardo.clone();
        let editor = cx.new(|cx| EditorScreen::new(bardo, project, window, cx));
        let subscription =
            cx.subscribe_in(&editor, window, |this, _, event, window, cx| match event {
                EditorEvent::Close => {
                    this.editor = None;
                    // The editor may have queued jobs or removed old proxies.
                    this.projects
                        .update(cx, |projects, cx| projects.reload(window, cx));
                    cx.notify();
                }
                EditorEvent::ToggleJobs => {
                    this.jobs_open = !this.jobs_open;
                    cx.notify();
                }
            });
        self.editor = Some((editor, subscription));
        cx.notify();
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
        if let Some((editor, _)) = &self.editor {
            return h_flex()
                .size_full()
                .items_start()
                .child(div().flex_1().h_full().min_w_0().child(editor.clone()))
                .when(self.jobs_open, |row| row.child(self.jobs.clone()))
                .into_any_element();
        }
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

        let screen_switch = ButtonGroup::new("screen")
            .small()
            .ghost()
            .children(Screen::ALL.map(|screen| {
                Button::new(("screen", screen as usize))
                    .label(tr(bardo, screen.title()))
                    .selected(screen == self.screen)
            }))
            .on_click(cx.listener(|this, clicked: &Vec<usize>, window, cx| {
                if let Some(screen) = clicked.first().and_then(|&i| Screen::ALL.get(i)) {
                    this.show(*screen, window, cx);
                }
            }));

        let top_bar = h_flex()
            .h(px(48.))
            .px_4()
            .gap_3()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(div().text_lg().child(tr(bardo, Text::AppName)))
            .child(screen_switch)
            // The tagline yields its space first on narrow windows.
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr(bardo, Text::AppTagline)),
            )
            .children(
                self.error
                    .map(|error| div().text_color(cx.theme().danger).child(tr(bardo, error))),
            )
            .child(
                Badge::new().count(self.jobs.read(cx).active()).child(
                    Button::new("toggle-jobs")
                        .small()
                        .ghost()
                        .selected(self.jobs_open)
                        .label(tr(bardo, Text::JobsTitle))
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.jobs_open = !this.jobs_open;
                            cx.notify();
                        })),
                ),
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
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_start()
                    .child(
                        div()
                            .flex_1()
                            .h_full()
                            .min_w_0()
                            .map(|main| match self.screen {
                                Screen::Channels => main.child(self.channels.clone()),
                                Screen::Personas => main.child(self.personas.clone()),
                                Screen::Research => main.child(self.research.clone()),
                                Screen::Themes => main.child(self.themes.clone()),
                                Screen::Projects => main.child(self.projects.clone()),
                                Screen::Templates => main.child(self.templates.clone()),
                                Screen::Costs => main.child(self.costs.clone()),
                                Screen::Settings => main.child(self.settings.clone()),
                            }),
                    )
                    .when(self.jobs_open, |row| row.child(self.jobs.clone())),
            )
            .into_any_element()
    }
}
