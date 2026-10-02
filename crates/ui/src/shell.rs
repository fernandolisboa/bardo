use bardo_app::bardo_domain::{BudgetLevel, VideoProjectId};
use bardo_app::{Bardo, Destination, SpendSummary, Stage, Text};
use gpui_kit::component::h_flex;
use gpui_kit::prelude::*;
use gpui_kit::{Entity, SharedString, Subscription, Window, div};

use crate::accounts::AccountsScreen;
use crate::appearance;
use crate::channels::ChannelsScreen;
use crate::costs::CostsScreen;
use crate::editor::{EditorEvent, EditorScreen};
use crate::jobs::JobsPanel;
use crate::kit::Tone;
use crate::layout;
use crate::parts::{BudgetMeter, Navigation};
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

/// The main window: the navigation, the current screen, and the jobs panel
/// beside it when open; where each goes is the layout's
/// ([`crate::layout`]). `Bardo` lives in an entity so screens re-render
/// when it changes (e.g. the language, set in Settings).
pub struct Shell {
    bardo: Entity<Bardo>,
    screen: Destination,
    channels: Entity<ChannelsScreen>,
    accounts: Entity<AccountsScreen>,
    personas: Entity<PersonasScreen>,
    research: Entity<ResearchScreen>,
    themes: Entity<ThemesScreen>,
    projects: Entity<ProjectsScreen>,
    templates: Entity<TemplatesScreen>,
    costs: Entity<CostsScreen>,
    settings: Entity<SettingsScreen>,
    /// Kept alive while closed, so the navigation's count stays current.
    jobs: Entity<JobsPanel>,
    jobs_open: bool,
    /// This month's spend for the navigation, and the job revision it was
    /// read at.
    spend: Option<SpendSummary>,
    spend_revision: u64,
    /// The editor, open over the whole window in place of the screens.
    editor: Option<(Entity<EditorScreen>, Subscription)>,
    _subscriptions: Vec<Subscription>,
}

impl Shell {
    pub fn new(bardo: Bardo, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let bardo = cx.new(|_| bardo);
        let channels = cx.new(|cx| ChannelsScreen::new(bardo.clone(), window, cx));
        let accounts = cx.new(|cx| AccountsScreen::new(bardo.clone(), window, cx));
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
            // A job that moved may have spent something.
            cx.observe(&jobs, |this, _, cx| {
                if this.bardo.read(cx).jobs_revision() != this.spend_revision {
                    this.refresh_spend(cx);
                }
                cx.notify();
            }),
            // Budgets change on the costs screen.
            cx.observe(&costs, |this, _, cx| {
                this.refresh_spend(cx);
                cx.notify();
            }),
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
        let mut shell = Self {
            bardo,
            screen: Destination::START,
            channels,
            accounts,
            personas,
            research,
            themes,
            projects,
            templates,
            costs,
            settings,
            jobs,
            jobs_open: false,
            spend: None,
            spend_revision: 0,
            editor: None,
            _subscriptions: subscriptions,
        };
        shell.refresh_spend(cx);
        shell
    }

    fn refresh_spend(&mut self, cx: &mut Context<Self>) {
        let bardo = self.bardo.read(cx);
        self.spend_revision = bardo.jobs_revision();
        self.spend = bardo
            .costs(bardo.current_month())
            .ok()
            .map(|view| view.summary());
    }

    /// Goes to `place`; Jobs opens or closes its panel beside the screen.
    fn pick(&mut self, place: Destination, window: &mut Window, cx: &mut Context<Self>) {
        if place == Destination::Jobs {
            self.jobs_open = !self.jobs_open;
            cx.notify();
            return;
        }
        // What a screen lists may have changed on another one: channels,
        // personas, niches, projects started from themes.
        if place != self.screen {
            match place {
                Destination::Channels => self
                    .channels
                    .update(cx, |channels, cx| channels.reload_personas(window, cx)),
                Destination::Accounts => self
                    .accounts
                    .update(cx, |accounts, cx| accounts.reload(window, cx)),
                Destination::Personas => self
                    .personas
                    .update(cx, |personas, cx| personas.reload(window, cx)),
                Destination::Research => self
                    .research
                    .update(cx, |research, cx| research.reload_channels(window, cx)),
                Destination::Themes => self
                    .themes
                    .update(cx, |themes, cx| themes.reload_channels(window, cx)),
                Destination::Projects => self
                    .projects
                    .update(cx, |projects, cx| projects.reload(window, cx)),
                Destination::Templates => self
                    .templates
                    .update(cx, |templates, cx| templates.reload(window, cx)),
                Destination::Costs => self.costs.update(cx, |costs, cx| costs.reload(cx)),
                Destination::Settings | Destination::Jobs => {}
            }
        }
        self.screen = place;
        self.refresh_spend(cx);
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
                    EditorEvent::Render => {
                        editor.update(cx, |editor, cx| editor.release(window, cx));
                        this.editor = None;
                        this.projects.update(cx, |projects, cx| {
                            projects.reload(window, cx);
                            projects.show_stage(Stage::Render, window, cx);
                        });
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

    fn navigation(&self, cx: &mut Context<Self>) -> Navigation {
        let shell = cx.entity().downgrade();
        let bardo = self.bardo.read(cx);
        let mut navigation = Navigation::new(bardo, self.screen, move |place, window, cx| {
            let _ = shell.update(cx, |shell, cx| shell.pick(place, window, cx));
        });
        navigation.jobs_open = self.jobs_open;
        navigation.jobs = self.jobs.read(cx).active();
        navigation.jobs_line = match navigation.jobs {
            0 => tr(bardo, Text::StatusNoJobs),
            1 => tr(bardo, Text::StatusOneJob),
            n => SharedString::from(bardo.text_with(Text::StatusJobs, &[("n", &n.to_string())])),
        };
        if let Some(spend) = &self.spend {
            navigation.spent = Some(SharedString::from(bardo.money(spend.total)));
            navigation.spent_line = Some(SharedString::from(bardo.text_with(
                Text::StatusMonthSpend,
                &[
                    ("amount", &bardo.money(spend.total)),
                    ("month", &bardo.month_name(bardo.current_month())),
                ],
            )));
            navigation.budgets = spend.budget_percent.map(|percent| BudgetMeter {
                percent,
                tone: match spend.level() {
                    BudgetLevel::Reached => Tone::Danger,
                    BudgetLevel::Warning => Tone::Warning,
                    BudgetLevel::Under => Tone::Accent,
                },
                line: SharedString::from(
                    bardo.text_with(Text::NavBudgetsUsed, &[("percent", &percent.to_string())]),
                ),
            });
        }
        navigation
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
        let navigation = self.navigation(cx);
        let screen = match self.screen {
            Destination::Channels => self.channels.clone().into_any_element(),
            Destination::Accounts => self.accounts.clone().into_any_element(),
            Destination::Personas => self.personas.clone().into_any_element(),
            Destination::Research => self.research.clone().into_any_element(),
            Destination::Themes => self.themes.clone().into_any_element(),
            Destination::Projects | Destination::Jobs => self.projects.clone().into_any_element(),
            Destination::Templates => self.templates.clone().into_any_element(),
            Destination::Costs => self.costs.clone().into_any_element(),
            Destination::Settings => self.settings.clone().into_any_element(),
        };
        let jobs = self.jobs_open.then(|| self.jobs.clone().into_any_element());
        layout::shell(navigation, screen, jobs, cx)
    }
}
