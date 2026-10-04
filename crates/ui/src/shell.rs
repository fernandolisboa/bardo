use std::rc::Rc;

use bardo_app::bardo_domain::{BudgetLevel, TourId, VideoProjectId};
use bardo_app::{
    Bardo, Destination, GuidePlace, GuideRef, ScreenTour, SpendSummary, Stage, Text, TourMove,
    TourPlace,
};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Entity, FocusHandle, ScrollHandle, SharedString, Subscription, Window, div, px,
};

use crate::accounts::AccountsScreen;
use crate::appearance::{self, look};
use crate::channels::ChannelsScreen;
use crate::costs::CostsScreen;
use crate::editor::{EditorEvent, EditorScreen};
use crate::guide::{Guide, GuideEvent, GuideHooks};
use crate::guide_screen::{GuideScreen, GuideScreenEvent, OpenGuide, TourThisScreen};
use crate::jobs::JobsPanel;
use crate::kit::Tone;
use crate::layout;
use crate::missed::{MissedChanged, MissedPosts};
use crate::network_accounts::OpenNetworkSettings;
use crate::parts::{BudgetMeter, Navigation};
use crate::performance::PerformanceScreen;
use crate::personas::PersonasScreen;
use crate::projects::{OpenEditor, ProjectsScreen};
use crate::research::ResearchScreen;
use crate::settings::SettingsScreen;
use crate::templates::TemplatesScreen;
use crate::themes::ThemesScreen;
use crate::title_bar::{self, TitleBar};

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
    performance: Entity<PerformanceScreen>,
    projects: Entity<ProjectsScreen>,
    templates: Entity<TemplatesScreen>,
    costs: Entity<CostsScreen>,
    settings: Entity<SettingsScreen>,
    /// Kept alive while closed, so the navigation's count stays current.
    jobs: Entity<JobsPanel>,
    jobs_open: bool,
    /// Scheduled posts that missed their time, over the window until the
    /// user decides on each or puts them off.
    missed: Entity<MissedPosts>,
    /// The first-run offer, the help menu and the guided tour, over the
    /// screens.
    guide: Entity<Guide>,
    /// The user guide, a screen of its own.
    guide_screen: Entity<GuideScreen>,
    /// The navigation's scroll, kept across frames so the tour can bring
    /// a place into view.
    nav_scroll: ScrollHandle,
    /// This month's spend for the navigation, and the job revision it was
    /// read at.
    spend: Option<SpendSummary>,
    spend_revision: u64,
    /// The editor, open over the whole window in place of the screens.
    editor: Option<(Entity<EditorScreen>, Subscription)>,
    /// The Guide screen shows over the open editor (F1, a tour card's
    /// "Learn more"); closing it goes back to the editor as it was.
    guide_over_editor: bool,
    /// Keeps the window's keys reaching the shell (F1) when nothing inside
    /// it has the keyboard.
    focus: FocusHandle,
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
        let performance = cx.new(|cx| PerformanceScreen::new(bardo.clone(), window, cx));
        let projects = cx.new(|cx| ProjectsScreen::new(bardo.clone(), window, cx));
        let templates = cx.new(|cx| TemplatesScreen::new(bardo.clone(), window, cx));
        let costs = cx.new(|cx| CostsScreen::new(bardo.clone(), window, cx));
        let settings = cx.new(|cx| SettingsScreen::new(bardo.clone(), window, cx));
        let jobs = cx.new(|cx| JobsPanel::new(bardo.clone(), cx));
        let missed = cx.new(|cx| MissedPosts::new(bardo.clone(), window, cx));
        let guide = cx.new(|cx| Guide::new(bardo.clone(), missed.clone(), cx));
        let guide_screen = cx.new(|cx| GuideScreen::new(bardo.clone(), window, cx));
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
                // A job that moved may have missed its post's time.
                this.missed.update(cx, |missed, cx| missed.jobs_moved(cx));
                cx.notify();
            }),
            // The Publish stage shows what the list decided.
            cx.subscribe_in(&missed, window, |this, _, _: &MissedChanged, window, cx| {
                this.projects
                    .update(cx, |projects, cx| projects.reload(window, cx));
                cx.notify();
            }),
            cx.observe(&missed, |_, _, cx| cx.notify()),
            cx.subscribe_in(&guide, window, |this, _, event: &GuideEvent, window, cx| {
                this.guide_event(*event, window, cx);
            }),
            // The Guide place shows whether its menu is open.
            cx.observe(&guide, |_, _, cx| cx.notify()),
            cx.subscribe_in(
                &guide_screen,
                window,
                |this, _, event: &GuideScreenEvent, window, cx| match *event {
                    GuideScreenEvent::Tour(tour) => {
                        this.guide_event(GuideEvent::Start(tour), window, cx);
                    }
                    GuideScreenEvent::Go(place) => this.go(place, window, cx),
                },
            ),
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
                &accounts,
                window,
                |this, _, _: &OpenNetworkSettings, window, cx| {
                    this.settings
                        .update(cx, |settings, cx| settings.show_networks(cx));
                    this.pick(Destination::Settings, window, cx);
                },
            ),
            cx.subscribe_in(
                &projects,
                window,
                |this, _, event: &OpenEditor, window, cx| {
                    this.open_editor(event.0, window, cx);
                },
            ),
        ];
        // A screen's "Tour this screen" and its ⓘ's "More in the guide".
        let tour_shell = cx.entity().downgrade();
        let section_shell = cx.entity().downgrade();
        cx.set_global(GuideHooks {
            tour: Rc::new(move |tour, window, cx| {
                let _ =
                    tour_shell.update(cx, |shell, cx| shell.start_screen_tour(tour, window, cx));
            }),
            section: Rc::new(move |section, window, cx| {
                let _ =
                    section_shell.update(cx, |shell, cx| shell.open_section(section, window, cx));
            }),
        });
        let mut shell = Self {
            bardo,
            screen: Destination::START,
            channels,
            accounts,
            personas,
            research,
            themes,
            performance,
            projects,
            templates,
            costs,
            settings,
            jobs,
            jobs_open: false,
            missed,
            guide,
            guide_screen,
            nav_scroll: ScrollHandle::new(),
            spend: None,
            spend_revision: 0,
            editor: None,
            guide_over_editor: false,
            focus: cx.focus_handle(),
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

    /// Goes to `place`; Jobs opens or closes its panel beside the screen,
    /// Guide its menu.
    fn pick(&mut self, place: Destination, window: &mut Window, cx: &mut Context<Self>) {
        if place == Destination::Jobs {
            self.jobs_open = !self.jobs_open;
            cx.notify();
            return;
        }
        if place == Destination::Guide {
            let screen_tour = self.screen_tour(cx);
            let stage_tour = self.stage_tour(cx);
            self.guide.update(cx, |guide, cx| {
                guide.toggle_menu(screen_tour, stage_tour, cx)
            });
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
                Destination::Performance => self
                    .performance
                    .update(cx, |performance, cx| performance.reload(window, cx)),
                Destination::Projects => self
                    .projects
                    .update(cx, |projects, cx| projects.reload(window, cx)),
                Destination::Templates => self
                    .templates
                    .update(cx, |templates, cx| templates.reload(window, cx)),
                Destination::Costs => self.costs.update(cx, |costs, cx| costs.reload(cx)),
                Destination::Settings | Destination::Jobs | Destination::Guide => {}
            }
        }
        self.screen = place;
        self.refresh_spend(cx);
        cx.notify();
    }

    /// Runs what the user asked of the Guide, and opens the place the
    /// tour's step is on.
    fn guide_event(&mut self, event: GuideEvent, window: &mut Window, cx: &mut Context<Self>) {
        // The missed posts are not drawn over the editor, so they hold
        // nothing back there.
        let missed_open = self.editor.is_none() && self.missed.read(cx).is_open();
        let from = self.screen;
        // In the editor, Resume tour is out of reach: its tours go back to
        // the step they closed on.
        let in_editor = self.editor.is_some();
        let moved = self.bardo.update(cx, |bardo, _| match event {
            GuideEvent::Start(tour @ (TourId::Editor | TourId::EditorMore)) if in_editor => {
                bardo.continue_tour(tour, from, missed_open).ok()
            }
            GuideEvent::Start(tour) => bardo.start_tour(tour, from, missed_open).ok(),
            GuideEvent::Resume => bardo.resume_tour(from, missed_open).ok(),
            GuideEvent::Next => Some(bardo.tour_next()),
            GuideEvent::Back => Some(bardo.tour_back()),
            GuideEvent::Skip => Some(bardo.tour_skip()),
            GuideEvent::Close => Some(bardo.tour_close()),
            // A frame drawn twice asks twice; only the step it saw goes.
            GuideEvent::StepOver(number) => bardo
                .tour_step(missed_open)
                .filter(|step| step.number == number)
                .map(|_| bardo.tour_step_over()),
            GuideEvent::Decline { never } => {
                bardo.decline_tour_offer(never);
                None
            }
            GuideEvent::Reset | GuideEvent::OpenGuide => None,
            // The tour closes as Esc closes it, so Resume tour goes back.
            GuideEvent::LearnMore(_) => Some(bardo.tour_close()),
        });
        if event == GuideEvent::Reset {
            let reset = self.bardo.update(cx, |bardo, _| bardo.reset_tours());
            if let Err(error) = reset {
                tracing::warn!(%error, "could not reset the tours");
                // The menu stays open and says so.
                self.guide.update(cx, |guide, cx| guide.reset_failed(cx));
                cx.notify();
                return;
            }
        }
        match moved {
            Some(TourMove::Show(Some(place))) => self.show_tour_place(place, window, cx),
            // A tour of no one place (the welcome) shows over the screens.
            Some(TourMove::Show(None)) if self.editor.is_some() => self.close_editor(window, cx),
            Some(TourMove::Left(origin)) if origin != self.screen => {
                self.show(origin, window, cx);
            }
            _ => {}
        }
        self.guide.update(cx, |guide, cx| guide.moved(cx));
        match event {
            GuideEvent::OpenGuide => self.open_guide(window, cx),
            GuideEvent::LearnMore(guide) => self.show_section(guide, cx),
            _ => {}
        }
        cx.notify();
    }

    /// Opens the place a tour's step is on. The editor's steps show over
    /// the editor, which holds still under them (opening it on the open
    /// project when it is closed); any other place sits under the editor,
    /// which closes first.
    fn show_tour_place(&mut self, place: TourPlace, window: &mut Window, cx: &mut Context<Self>) {
        if place == TourPlace::Stage(Stage::Edit) {
            self.guide_over_editor = false;
            match &self.editor {
                Some((editor, _)) => editor.update(cx, |editor, cx| {
                    editor.hold_for_tour(cx);
                    // From the guide over it, the keyboard was in the
                    // guide's search box: the card gives it back here.
                    editor.focus(window, cx);
                }),
                None => {
                    self.pick(Destination::Projects, window, cx);
                    self.projects.update(cx, |projects, cx| {
                        projects.show_stage(Stage::Edit, window, cx)
                    });
                }
            }
            return;
        }
        self.close_editor(window, cx);
        match place {
            TourPlace::Screen(place) => self.pick(place, window, cx),
            TourPlace::Stage(stage) => {
                self.pick(Destination::Projects, window, cx);
                self.projects
                    .update(cx, |projects, cx| projects.show_stage(stage, window, cx));
            }
        }
    }

    /// Shows `place`; unlike a pick in the navigation, the Guide shows its
    /// screen rather than its menu.
    fn show(&mut self, place: Destination, window: &mut Window, cx: &mut Context<Self>) {
        if place == Destination::Guide {
            self.screen = Destination::Guide;
            cx.notify();
        } else {
            self.pick(place, window, cx);
        }
    }

    /// Where the user is, as the guide names places: the open project's
    /// stage, the Settings tab, or the screen.
    fn place(&self, cx: &App) -> Option<GuidePlace> {
        match self.screen {
            Destination::Guide => None,
            Destination::Projects => Some(
                self.projects
                    .read(cx)
                    .current_stage()
                    .map_or(GuidePlace::Screen(Destination::Projects), GuidePlace::Stage),
            ),
            Destination::Settings => Some(GuidePlace::Settings(self.settings.read(cx).tab())),
            place => Some(GuidePlace::Screen(place)),
        }
    }

    /// F1 or the menu's "User guide": the Guide screen at the page for
    /// where the user is (staying on the open page when already there),
    /// with the keyboard in its search box.
    fn open_guide(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editor.is_some() {
            if self.guide_over_editor {
                self.guide_screen
                    .update(cx, |screen, cx| screen.focus_search(window, cx));
            } else {
                self.cover_editor(cx);
                let place = Some(GuidePlace::Stage(Stage::Edit));
                self.guide_screen
                    .update(cx, |screen, cx| screen.open_at(place, window, cx));
            }
            cx.notify();
            return;
        }
        if self.screen == Destination::Guide {
            self.guide_screen
                .update(cx, |screen, cx| screen.focus_search(window, cx));
            return;
        }
        let place = self.place(cx);
        self.guide_screen
            .update(cx, |screen, cx| screen.open_at(place, window, cx));
        self.screen = Destination::Guide;
        cx.notify();
    }

    /// F1. Not while the missed posts list holds the window; over the
    /// editor the guide opens on top of it; during a tour it is the step's
    /// "Learn more".
    fn on_open_guide(&mut self, _: &OpenGuide, window: &mut Window, cx: &mut Context<Self>) {
        if self.editor.is_none() && self.missed.read(cx).is_open() {
            return;
        }
        let step = self.bardo.read(cx).tour_step(false);
        match step {
            Some(step) => {
                if let Some(guide) = step.guide {
                    self.guide_event(GuideEvent::LearnMore(guide), window, cx);
                }
            }
            None => {
                self.guide.update(cx, |guide, cx| guide.close_cards(cx));
                self.open_guide(window, cx);
            }
        }
    }

    /// The tour the screen on display offers: one with a tour of its own
    /// and something to show.
    fn screen_tour(&self, cx: &App) -> Option<ScreenTour> {
        let has_content = match self.screen {
            Destination::Research => self.research.read(cx).has_content(),
            Destination::Themes => self.themes.read(cx).has_content(),
            Destination::Performance => self.performance.read(cx).has_content(),
            Destination::Projects => self.projects.read(cx).has_content(),
            Destination::Personas => self.personas.read(cx).has_content(),
            Destination::Templates => self.templates.read(cx).has_content(),
            _ => false,
        };
        self.bardo.read(cx).screen_tour(self.screen, has_content)
    }

    /// The tour the open project's stage on display offers: one with a
    /// tour of its own that has made something.
    fn stage_tour(&self, cx: &App) -> Option<ScreenTour> {
        if self.screen != Destination::Projects {
            return None;
        }
        let (stage, has_content) = self.projects.read(cx).stage_content()?;
        self.bardo.read(cx).stage_tour(stage, has_content)
    }

    /// Shift+F1: the tour of the stage on screen, else the screen's, when
    /// one is offered; in the editor, the editor's. Not while the missed
    /// posts list holds the window.
    fn on_tour_this_screen(
        &mut self,
        _: &TourThisScreen,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Not mid-tour: a running tour keeps its step.
        if self.bardo.read(cx).tour_step(false).is_some() {
            return;
        }
        if let Some((editor, _)) = &self.editor {
            let has_content = editor.read(cx).has_content();
            if let Some(tour) = self.bardo.read(cx).stage_tour(Stage::Edit, has_content) {
                self.start_screen_tour(tour.tour, window, cx);
            }
            return;
        }
        if self.missed.read(cx).is_open() {
            return;
        }
        if let Some(tour) = self.stage_tour(cx).or_else(|| self.screen_tour(cx)) {
            self.start_screen_tour(tour.tour, window, cx);
        }
    }

    /// "Tour this screen": the cards over the screen close first.
    fn start_screen_tour(&mut self, tour: TourId, window: &mut Window, cx: &mut Context<Self>) {
        self.guide.update(cx, |guide, cx| guide.close_cards(cx));
        self.guide_event(GuideEvent::Start(tour), window, cx);
    }

    /// An ⓘ's "More in the guide": the Guide screen at that section.
    fn open_section(&mut self, section: GuideRef, _: &mut Window, cx: &mut Context<Self>) {
        self.guide.update(cx, |guide, cx| guide.close_cards(cx));
        self.show_section(section, cx);
        cx.notify();
    }

    /// The Guide screen at `section`, over the editor when it is open.
    fn show_section(&mut self, section: GuideRef, cx: &mut Context<Self>) {
        self.guide_screen.update(cx, |screen, cx| {
            screen.open(section.page, Some(section.section), cx);
        });
        if self.editor.is_some() {
            self.cover_editor(cx);
        } else {
            self.screen = Destination::Guide;
        }
    }

    /// The guide goes over the editor, which is no longer drawn: playback
    /// pauses rather than play on unseen.
    fn cover_editor(&mut self, cx: &mut Context<Self>) {
        self.guide_over_editor = true;
        if let Some((editor, _)) = &self.editor {
            editor.update(cx, |editor, cx| editor.hold_for_tour(cx));
        }
    }

    /// A guide link or "Go to …": opens the place. From the guide over the
    /// editor, the editor itself is back where it was; any other place
    /// closes it.
    fn go(&mut self, place: GuidePlace, window: &mut Window, cx: &mut Context<Self>) {
        if self.editor.is_some() {
            if place == GuidePlace::Stage(Stage::Edit) {
                self.back_to_editor(window, cx);
                return;
            }
            self.close_editor(window, cx);
        }
        match place {
            GuidePlace::Screen(place) => self.show(place, window, cx),
            GuidePlace::Stage(stage) => {
                self.pick(Destination::Projects, window, cx);
                self.projects
                    .update(cx, |projects, cx| projects.show_stage(stage, window, cx));
            }
            GuidePlace::Settings(tab) => {
                self.settings
                    .update(cx, |settings, cx| settings.show_tab(tab, cx));
                self.pick(Destination::Settings, window, cx);
            }
        }
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
                EditorEvent::Close => this.close_editor(window, cx),
                EditorEvent::Render => {
                    this.close_editor(window, cx);
                    this.projects.update(cx, |projects, cx| {
                        projects.show_stage(Stage::Render, window, cx)
                    });
                }
                EditorEvent::ToggleJobs => {
                    this.jobs_open = !this.jobs_open;
                    cx.notify();
                }
            });
        self.editor = Some((editor, subscription));
        self.guide_over_editor = false;
        self.guide
            .update(cx, |guide, cx| guide.set_over_editor(true, cx));
        cx.notify();
    }

    /// Back to the projects screen: the editor lets go of its preview.
    fn close_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((editor, _)) = self.editor.take() else {
            return;
        };
        editor.update(cx, |editor, cx| editor.release(window, cx));
        self.guide_over_editor = false;
        self.guide
            .update(cx, |guide, cx| guide.set_over_editor(false, cx));
        // The editor may have queued jobs or removed old proxies.
        self.projects
            .update(cx, |projects, cx| projects.reload(window, cx));
        cx.notify();
    }

    /// Closes the guide over the editor: the editor is as it was, with the
    /// keyboard.
    fn back_to_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.guide_over_editor = false;
        if let Some((editor, _)) = &self.editor {
            editor.update(cx, |editor, cx| editor.focus(window, cx));
        }
        cx.notify();
    }

    fn navigation(&self, cx: &mut Context<Self>) -> Navigation {
        let shell = cx.entity().downgrade();
        let bardo = self.bardo.read(cx);
        let mut navigation = Navigation::new(bardo, self.screen, move |place, window, cx| {
            let _ = shell.update(cx, |shell, cx| shell.pick(place, window, cx));
        });
        navigation.jobs_open = self.jobs_open;
        navigation.guide_open = self.guide.read(cx).is_menu_open();
        navigation.scroll = self.nav_scroll.clone();
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The tour's anchors are recorded afresh as this frame prepaints.
        crate::tour::begin_frame(window, cx);
        // Keys go to the root alone while nothing on screen has the keyboard
        // (nothing focused yet, or the focused button was on a screen that
        // is gone), which would miss the shell's F1. Everything Bardo draws,
        // popovers included, sits inside the shell; a dialog opened through
        // the kit's root (`open_dialog`, `open_sheet`) would sit beside it
        // and lose the keyboard to this.
        if !self.focus.contains_focused(window, cx) {
            let focus = self.focus.clone();
            window.on_next_frame(move |window, cx| {
                if !focus.contains_focused(window, cx) {
                    window.focus(&focus, cx);
                }
            });
        }
        let name = tr(self.bardo.read(cx), Text::AppName);
        let (colors, content) = match &self.editor {
            Some(_) if self.guide_over_editor => (
                title_bar::Colors::interface(cx),
                self.render_guide_over_editor(cx),
            ),
            Some((editor, _)) => (title_bar::Colors::editor(), self.render_editor(editor)),
            None => (title_bar::Colors::interface(cx), self.render_screens(cx)),
        };
        // Over the editor, the Guide shows the editor's tours alone.
        let guide = self.guide.clone();
        v_flex()
            .size_full()
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::on_open_guide))
            .on_action(cx.listener(Self::on_tour_this_screen))
            .child(TitleBar::new(name, colors))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(content)
                    .child(guide),
            )
    }
}

impl Shell {
    /// The editor over the whole window, the jobs panel beside it when open.
    fn render_editor(&self, editor: &Entity<EditorScreen>) -> AnyElement {
        h_flex()
            .size_full()
            .items_start()
            .child(div().flex_1().h_full().min_w_0().child(editor.clone()))
            .when(self.jobs_open, |row| row.child(self.jobs.clone()))
            .into_any_element()
    }

    /// The Guide screen over the editor, under a bar that goes back to it.
    fn render_guide_over_editor(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = look(cx).tokens;
        let back = Button::new("guide-back-to-editor")
            .ghost()
            .small()
            .icon(IconName::ChevronLeft)
            .label(tr(self.bardo.read(cx), Text::GuideBackToEditor))
            .on_click(cx.listener(|this, _, window, cx| this.back_to_editor(window, cx)));
        v_flex()
            .size_full()
            .bg(t.app)
            .child(
                h_flex()
                    .h(px(40.))
                    .flex_none()
                    .px_3()
                    .items_center()
                    .border_b(t.border_width)
                    .border_color(t.border)
                    .child(back),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(self.guide_screen.clone()),
            )
            .into_any_element()
    }

    /// The current screen in the layout, under the missed posts when any.
    fn render_screens(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let navigation = self.navigation(cx);
        let screen = match self.screen {
            Destination::Channels => self.channels.clone().into_any_element(),
            Destination::Accounts => self.accounts.clone().into_any_element(),
            Destination::Personas => self.personas.clone().into_any_element(),
            Destination::Research => self.research.clone().into_any_element(),
            Destination::Themes => self.themes.clone().into_any_element(),
            Destination::Performance => self.performance.clone().into_any_element(),
            Destination::Guide => self.guide_screen.clone().into_any_element(),
            Destination::Projects | Destination::Jobs => self.projects.clone().into_any_element(),
            Destination::Templates => self.templates.clone().into_any_element(),
            Destination::Costs => self.costs.clone().into_any_element(),
            Destination::Settings => self.settings.clone().into_any_element(),
        };
        let jobs = self.jobs_open.then(|| self.jobs.clone().into_any_element());
        let shell = layout::shell(navigation, screen, jobs, cx);
        if !self.missed.read(cx).is_open() {
            return shell;
        }
        div()
            .relative()
            .size_full()
            .child(shell)
            .child(self.missed.clone())
            .into_any_element()
    }
}
