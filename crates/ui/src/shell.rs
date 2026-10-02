use bardo_app::bardo_domain::VideoProjectId;
use bardo_app::{Bardo, Text};
use gpui_kit::component::badge::Badge;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Selectable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{ClickEvent, Entity, SharedString, Subscription, Window, div, px};

use crate::appearance::{self, look};
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

/// The main window: top bar with the screen switch and the jobs toggle,
/// the current screen below and the jobs panel on the right when open.
/// `Bardo` lives in an entity so screens re-render when it changes (e.g.
/// the language, set in Settings).
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
        // The startup theme guessed the system's appearance before any
        // window existed; this window knows it.
        appearance::follow(
            bardo.read(cx).ui_theme(),
            appearance::system_mode(window),
            cx,
        );
        let subscriptions = vec![
            cx.observe(&jobs, |_, _, cx| cx.notify()),
            // The title follows the interface language.
            cx.observe_in(&bardo, window, |_, bardo, window, cx| {
                window.set_window_title(&bardo.read(cx).text(Text::AppName));
            }),
            // "Follow Windows" switches with the system's light/dark setting.
            cx.observe_window_appearance(window, |this, window, cx| {
                let preference = this.bardo.read(cx).ui_theme();
                appearance::follow(preference, appearance::system_mode(window), cx);
            }),
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
            cx.subscribe_in(
                &editor,
                window,
                |this, editor, event, window, cx| match event {
                    EditorEvent::Close => {
                        editor.update(cx, |editor, cx| editor.release(window, cx));
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
                },
            );
        self.editor = Some((editor, subscription));
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
        let t = look(cx).tokens;
        let nav = h_flex().gap_0p5().children(Screen::ALL.map(|screen| {
            let on = screen == self.screen;
            div()
                .id(("screen", screen as usize))
                .px_2p5()
                .py_1()
                .rounded(t.radius)
                .text_sm()
                .font_weight(gpui_kit::FontWeight::MEDIUM)
                .cursor_pointer()
                .border_b_2()
                .map(|item| {
                    if on {
                        item.bg(t.selected)
                            .text_color(t.text)
                            .border_color(t.accent)
                    } else {
                        item.text_color(t.text2)
                            .border_color(gpui_kit::transparent_black())
                            .hover(|item| item.bg(t.hover).text_color(t.text))
                    }
                })
                .child(tr(bardo, screen.title()))
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.show(screen, window, cx);
                }))
        }));

        let top_bar = h_flex()
            .h(px(48.))
            .px_4()
            .gap_3()
            .bg(t.surface)
            .border_b(t.border_width)
            .border_color(t.border)
            .child(
                div()
                    .text_lg()
                    .font_weight(gpui_kit::FontWeight::BOLD)
                    .mr_2()
                    .child(tr(bardo, Text::AppName)),
            )
            .child(nav)
            .child(div().flex_1())
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
            );

        v_flex()
            .size_full()
            .bg(t.app)
            .text_color(t.text)
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
