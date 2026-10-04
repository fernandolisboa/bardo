//! The Guide (issue #105): the first-run offer, the help menu behind the
//! Guide place, the keyboard shortcuts, and the guided tour on screen. All
//! of it is drawn deferred over the content area, after everything else.
//! The user guide itself is a screen of its own ([`crate::guide_screen`]),
//! opened from the menu, F1 or a tour card's "Learn more".
//! The tour's moves go to the shell as [`GuideEvent`]s, since some of them
//! open a place; what the tour is and where it stands is `bardo_app`'s.

use std::cell::RefCell;
use std::rc::Rc;

use bardo_app::bardo_domain::{ThemeFamily, TourId};
use bardo_app::{
    Bardo, Destination, GuideRef, SHORTCUTS, ScreenTour, Spot, Stage, Text, TourAnchor, TourPlace,
    TourStepView,
};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::{Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, ElementId, Entity, EventEmitter, FocusHandle, Global,
    KeyDownEvent, SharedString, Window, deferred, div, px,
};

use crate::appearance::look;
use crate::icons::Lucide;
use crate::kit;
use crate::missed::MissedPosts;
use crate::parts::{KeysHint, OnPick};
use crate::shell::tr;
use crate::tour::{Motion, Ring, Spotlight};

/// Above every other deferred drawing: popovers, menus, the missed posts.
const PRIORITY: usize = 10_000;
const CARD_WIDTH: f32 = 360.;
const OFFER_WIDTH: f32 = 420.;
const MENU_WIDTH: f32 = 260.;
const SHORTCUTS_WIDTH: f32 = 560.;

/// What the user asked of the Guide; the shell runs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuideEvent {
    Start(TourId),
    Resume,
    Next,
    Back,
    Skip,
    /// Esc: closes and keeps the step for "Resume tour".
    Close,
    /// The step's component is not on screen and the step goes. Carries
    /// the step's number, so a frame drawn twice passes over it once.
    StepOver(usize),
    /// "Not now" (false) or "Don't show again" (true).
    Decline {
        never: bool,
    },
    Reset,
    /// "User guide": the Guide screen, at the page for where the user is.
    OpenGuide,
    /// A tour card's "Learn more": the tour closes (Resume tour goes back
    /// to it) and the guide opens at the step's section.
    LearnMore(GuideRef),
}

/// What a screen's own controls ask of the Guide: start the screen's tour
/// ("Tour this screen"), or open the guide at a section (an ⓘ's "More in
/// the guide"). The shell sets them, so a screen needs no handle on it.
pub struct GuideHooks {
    pub tour: OnPick<TourId>,
    pub section: OnPick<GuideRef>,
}

impl Global for GuideHooks {}

/// Starts `tour` through the shell, as a screen's tour button does.
pub fn start_tour(tour: TourId, window: &mut Window, cx: &mut App) {
    if let Some(start) = cx
        .try_global::<GuideHooks>()
        .map(|hooks| hooks.tour.clone())
    {
        start(tour, window, cx);
    }
}

fn open_section(section: GuideRef, window: &mut Window, cx: &mut App) {
    if let Some(open) = cx
        .try_global::<GuideHooks>()
        .map(|hooks| hooks.section.clone())
    {
        open(section, window, cx);
    }
}

/// The guide sections the screens' ⓘs open. A test checks that each one
/// resolves in every language; add new ones here and to [`refs::ALL`].
pub mod refs {
    use bardo_app::GuideRef;

    const fn at(page: &'static str, section: &'static str) -> GuideRef {
        GuideRef { page, section }
    }

    pub const RESEARCH_SEEDS: GuideRef = at("niche-research", "seeds");
    pub const RESEARCH_RUN: GuideRef = at("niche-research", "run");
    pub const RESEARCH_SCORES: GuideRef = at("niche-research", "scores");
    pub const THEMES_SUGGEST: GuideRef = at("themes-ranking", "suggest");
    pub const THEMES_REASONS: GuideRef = at("themes-ranking", "reasons");
    pub const PERFORMANCE_LINK: GuideRef = at("performance-metrics", "link");
    pub const PERFORMANCE_POSTS: GuideRef = at("performance-metrics", "posts");
    pub const PERFORMANCE_NUMBERS: GuideRef = at("performance-metrics", "numbers");
    pub const PERFORMANCE_SYNC: GuideRef = at("performance-metrics", "sync");
    pub const PROJECTS_NARRATOR: GuideRef = at("projects", "narrator");
    pub const SCRIPT_WRITE: GuideRef = at("script", "write");
    pub const SCRIPT_REVIEW: GuideRef = at("script", "review");
    pub const SCRIPT_MUSIC: GuideRef = at("script", "music");
    pub const NARRATION_GENERATE: GuideRef = at("narration", "generate");
    pub const NARRATION_PLAY: GuideRef = at("narration", "play");
    pub const NARRATION_STALE: GuideRef = at("narration", "stale");
    pub const NARRATION_IMPORT: GuideRef = at("narration", "import");
    pub const SCENES_PLAN: GuideRef = at("scenes", "plan");
    pub const SCENES_REDRAW: GuideRef = at("scenes", "redraw");
    pub const SCENES_REPLAN: GuideRef = at("scenes", "replan");
    pub const CLIPS_ANIMATE: GuideRef = at("clips", "animate");
    pub const CLIPS_REVIEW: GuideRef = at("clips", "review");
    pub const PERSONAS_LIBRARY: GuideRef = at("personas", "library");
    pub const PERSONAS_VOICE: GuideRef = at("personas", "voice");
    pub const PERSONAS_PRESETS: GuideRef = at("personas", "presets");
    pub const PERSONAS_SAMPLE: GuideRef = at("personas", "sample");
    pub const PERSONAS_SHARE: GuideRef = at("personas", "share");
    pub const PERSONAS_REALISTIC: GuideRef = at("personas", "realistic");
    pub const TEMPLATES_VERSIONS: GuideRef = at("templates", "versions");
    pub const TEMPLATES_FIELDS: GuideRef = at("templates", "fields");
    pub const TEMPLATES_VARIABLES: GuideRef = at("templates", "variables");
    pub const RENDER_TARGETS: GuideRef = at("render", "targets");
    pub const RENDER_LAST: GuideRef = at("render", "last");
    pub const CHANNELS_PERSONA: GuideRef = at("channels", "persona");
    pub const CHANNELS_DEFAULTS: GuideRef = at("channels", "defaults");
    pub const ACCOUNTS_ONE: GuideRef = at("network-accounts", "accounts");
    pub const ACCOUNTS_METADATA: GuideRef = at("network-accounts", "metadata");
    pub const ACCOUNTS_PRESET: GuideRef = at("network-accounts", "preset");
    pub const CREDENTIALS_KEPT: GuideRef = at("app-credentials", "kept");
    pub const UPLOADING_NETWORKS: GuideRef = at("uploading", "networks");
    pub const UPLOADING_METADATA: GuideRef = at("uploading", "metadata");
    pub const UPLOADING_DISCLOSURE: GuideRef = at("uploading", "disclosure");
    pub const UPLOADING_REVIEW: GuideRef = at("uploading", "review");
    pub const UPLOAD_STATES: GuideRef = at("uploading", "states");
    pub const UPLOADING_POST: GuideRef = at("uploading", "post");
    pub const EXPORTING_OUTDATED: GuideRef = at("exporting", "outdated");
    pub const YOUTUBE_UPLOAD: GuideRef = at("connect-youtube", "upload");
    pub const INSTAGRAM_UPLOAD: GuideRef = at("connect-instagram", "upload");
    pub const COSTS_MONTH: GuideRef = at("costs", "month");
    pub const COSTS_BUDGETS: GuideRef = at("costs", "budgets");
    pub const COSTS_RATES: GuideRef = at("costs", "rates");
    pub const JOBS_TEST: GuideRef = at("jobs", "test");
    pub const KEYS_KEPT: GuideRef = at("api-keys", "where-kept");
    pub const SETTINGS_LAYOUT: GuideRef = at("settings", "layout");
    pub const SETTINGS_TOURS: GuideRef = at("settings", "tours");
    pub const METRICS_ON_START: GuideRef = at("metrics-sync", "on-start");
    pub const SHORTCUTS_LISTS: GuideRef = at("shortcuts", "lists");
    pub const SHORTCUTS_GUIDE: GuideRef = at("shortcuts", "guide");

    #[cfg(test)]
    pub const ALL: [GuideRef; 58] = [
        RESEARCH_SEEDS,
        RESEARCH_RUN,
        RESEARCH_SCORES,
        THEMES_SUGGEST,
        THEMES_REASONS,
        PERFORMANCE_LINK,
        PERFORMANCE_POSTS,
        PERFORMANCE_NUMBERS,
        PERFORMANCE_SYNC,
        PROJECTS_NARRATOR,
        SCRIPT_WRITE,
        SCRIPT_REVIEW,
        SCRIPT_MUSIC,
        NARRATION_GENERATE,
        NARRATION_PLAY,
        NARRATION_STALE,
        NARRATION_IMPORT,
        SCENES_PLAN,
        SCENES_REDRAW,
        SCENES_REPLAN,
        CLIPS_ANIMATE,
        CLIPS_REVIEW,
        PERSONAS_LIBRARY,
        PERSONAS_VOICE,
        PERSONAS_PRESETS,
        PERSONAS_SAMPLE,
        PERSONAS_SHARE,
        PERSONAS_REALISTIC,
        TEMPLATES_VERSIONS,
        TEMPLATES_FIELDS,
        TEMPLATES_VARIABLES,
        RENDER_TARGETS,
        RENDER_LAST,
        CHANNELS_PERSONA,
        CHANNELS_DEFAULTS,
        ACCOUNTS_ONE,
        ACCOUNTS_METADATA,
        ACCOUNTS_PRESET,
        CREDENTIALS_KEPT,
        UPLOADING_NETWORKS,
        UPLOADING_METADATA,
        UPLOADING_DISCLOSURE,
        UPLOADING_REVIEW,
        UPLOAD_STATES,
        UPLOADING_POST,
        EXPORTING_OUTDATED,
        YOUTUBE_UPLOAD,
        INSTAGRAM_UPLOAD,
        COSTS_MONTH,
        COSTS_BUDGETS,
        COSTS_RATES,
        JOBS_TEST,
        KEYS_KEPT,
        SETTINGS_LAYOUT,
        SETTINGS_TOURS,
        METRICS_ON_START,
        SHORTCUTS_LISTS,
        SHORTCUTS_GUIDE,
    ];
}

/// A screen header's info slot: its ⓘ, then "Tour this screen" when the
/// screen offers its tour (it has a tour and something to show), with the
/// "new" mark while the user has not taken it.
pub fn header_info(
    bardo: &Bardo,
    screen: Destination,
    has_content: bool,
    info: Option<AnyElement>,
    cx: &App,
) -> Option<AnyElement> {
    header_tours(bardo, (screen, has_content), None, info, cx)
}

/// [`header_info`] for the Projects screen, which also offers the tour of
/// the stage on screen ("Tour this stage") once the stage has made
/// something. Shift+F1 starts the stage's tour then.
pub fn header_tours(
    bardo: &Bardo,
    (screen, has_content): (Destination, bool),
    stage: Option<(Stage, bool)>,
    info: Option<AnyElement>,
    cx: &App,
) -> Option<AnyElement> {
    let screen_tour = bardo.screen_tour(screen, has_content);
    let stage_tour = stage.and_then(|(stage, has_content)| bardo.stage_tour(stage, has_content));
    if screen_tour.is_none() && stage_tour.is_none() {
        return info;
    }
    // The key goes with the tour Shift+F1 starts: the stage's, else the
    // screen's.
    let screen_key = stage_tour.is_none();
    Some(
        h_flex()
            .gap_1()
            .items_center()
            .children(info)
            .children(screen_tour.map(|tour| {
                tour_button(
                    bardo,
                    "tour-this-screen",
                    Text::TourThisScreen,
                    tour,
                    screen_key,
                    cx,
                )
            }))
            .children(stage_tour.map(|tour| {
                tour_button(
                    bardo,
                    "tour-this-stage",
                    Text::TourThisStage,
                    tour,
                    true,
                    cx,
                )
            }))
            .into_any_element(),
    )
}

/// A header's info slot for a place that is not a screen of its own (a
/// Settings tab): its ⓘ, then "Tour this screen" when the place has a tour.
pub fn place_info(
    bardo: &Bardo,
    place: TourPlace,
    info: Option<AnyElement>,
    cx: &App,
) -> Option<AnyElement> {
    let Some(tour) = bardo.place_tour(place, true) else {
        return info;
    };
    Some(
        h_flex()
            .gap_1()
            .items_center()
            .children(info)
            .child(tour_button(
                bardo,
                "tour-this-screen",
                Text::TourThisScreen,
                tour,
                true,
                cx,
            ))
            .into_any_element(),
    )
}

/// The Jobs panel's "Tour this panel", once the panel has jobs to show.
/// The panel sits beside the screen, so Shift+F1 stays the screen's.
pub fn panel_tour(bardo: &Bardo, has_content: bool, cx: &App) -> Option<AnyElement> {
    let tour = bardo.place_tour(TourPlace::Screen(Destination::Jobs), has_content)?;
    Some(
        tour_button(
            bardo,
            "tour-this-panel",
            Text::TourThisPanel,
            tour,
            false,
            cx,
        )
        .into_any_element(),
    )
}

/// The missed posts list's "Tour this list", the one tour that runs while
/// the list is up (Shift+F1 there). Hidden while another tour waits behind
/// the list, which Shift+F1 leaves alone too.
pub fn missed_posts_tour(bardo: &Bardo, cx: &App) -> Option<AnyElement> {
    if bardo.tour_step(false).is_some() {
        return None;
    }
    let tour = bardo.place_tour(TourPlace::Missed, true)?;
    Some(
        tour_button(bardo, "tour-this-list", Text::TourThisList, tour, true, cx).into_any_element(),
    )
}

fn tour_button(
    bardo: &Bardo,
    id: &'static str,
    label: Text,
    tour: ScreenTour,
    key: bool,
    cx: &App,
) -> impl IntoElement {
    let started = tour.tour;
    let button = Button::new(id)
        .ghost()
        .xsmall()
        .icon(Lucide::BookOpen)
        .label(tr(bardo, label))
        .on_click(move |_, window, cx| start_tour(started, window, cx));
    h_flex()
        .gap_1()
        .items_center()
        .child(if key {
            button.tooltip("Shift+F1")
        } else {
            button
        })
        .when(tour.new, |row| {
            row.child(kit::status(kit::Tone::Accent, tr(bardo, Text::TourNew), cx))
        })
}

/// An ⓘ whose popover ends with "More in the guide", opening `section`
/// (one of [`refs`]).
pub fn info(
    bardo: &Bardo,
    id: impl Into<ElementId>,
    text: SharedString,
    section: GuideRef,
) -> Popover {
    labeled_info(bardo, id, None, text, section)
}

/// [`info`] with a label beside the ⓘ ("Where credentials are kept").
pub fn labeled_info(
    bardo: &Bardo,
    id: impl Into<ElementId>,
    label: Option<SharedString>,
    text: SharedString,
    section: GuideRef,
) -> Popover {
    kit::info_more(
        id,
        label,
        text,
        tr(bardo, Text::GuideMoreInGuide),
        move |window, cx| {
            open_section(section, window, cx);
        },
    )
}

/// What the keys over a collection do, for its ⓘ, with the guide
/// `section` that says more.
pub fn keys_hint(bardo: &Bardo, text: Text, section: GuideRef) -> KeysHint {
    KeysHint {
        text: tr(bardo, text),
        more: tr(bardo, Text::GuideMoreInGuide),
        section,
    }
}

/// The ⓘ of a [`KeysHint`], drawn by the layout that shows it.
pub fn keys_info(id: impl Into<ElementId>, hint: KeysHint) -> Popover {
    let section = hint.section;
    kit::info_more(id, None, hint.text, hint.more, move |window, cx| {
        open_section(section, window, cx);
    })
}

/// A row of the help menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuItem {
    UserGuide,
    Tour(TourId),
    /// The current screen's tour (Shift+F1 unless the stage has one).
    ScreenTour(TourId),
    /// The tour of the open project's stage on screen (Shift+F1).
    StageTour(TourId),
    Resume,
    Shortcuts,
    Reset,
}

/// What a card's button does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Act {
    Send(GuideEvent),
    CloseShortcuts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Primary,
    Outline,
    Ghost,
}

/// What the Guide shows, most urgent first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layer {
    Tour(TourStepView),
    Shortcuts,
    Menu(usize),
    Offer,
}

pub struct Guide {
    bardo: Entity<Bardo>,
    missed: Entity<MissedPosts>,
    focus: FocusHandle,
    /// Where focus was before a card took it, given back when it closes.
    restore: Option<FocusHandle>,
    /// What was shown last frame, so focus moves only when it changes.
    shown: Option<Layer>,
    /// The help menu, with its highlighted row, while open.
    menu: Option<usize>,
    /// The tour the screen under the menu offers.
    screen_tour: Option<ScreenTour>,
    /// The tour the project stage under the menu offers.
    stage_tour: Option<ScreenTour>,
    shortcuts: bool,
    /// The card button the keyboard is on.
    button: Option<usize>,
    motion: Rc<RefCell<Motion>>,
    /// Why the last menu action failed.
    problem: Option<Text>,
    /// Over the editor only its tours show: no offer, menu or shortcuts
    /// card, and the missed posts, not drawn there, hold nothing back.
    over_editor: bool,
}

impl EventEmitter<GuideEvent> for Guide {}

impl Guide {
    pub fn new(bardo: Entity<Bardo>, missed: Entity<MissedPosts>, cx: &mut Context<Self>) -> Self {
        Self {
            bardo,
            missed,
            focus: cx.focus_handle(),
            restore: None,
            shown: None,
            menu: None,
            screen_tour: None,
            stage_tour: None,
            shortcuts: false,
            button: None,
            motion: Rc::default(),
            problem: None,
            over_editor: false,
        }
    }

    /// The editor opened over the screens (`true`) or closed.
    pub fn set_over_editor(&mut self, over: bool, cx: &mut Context<Self>) {
        if self.over_editor != over {
            self.over_editor = over;
            self.menu = None;
            self.shortcuts = false;
            cx.notify();
        }
    }

    pub fn is_menu_open(&self) -> bool {
        self.menu.is_some()
    }

    /// The Guide place was picked: opens or closes its menu, which offers
    /// `screen_tour` and `stage_tour`, the tours of the screen and the
    /// project stage under it.
    pub fn toggle_menu(
        &mut self,
        screen_tour: Option<ScreenTour>,
        stage_tour: Option<ScreenTour>,
        cx: &mut Context<Self>,
    ) {
        self.menu = match self.menu {
            Some(_) => None,
            None => Some(0),
        };
        self.screen_tour = screen_tour;
        self.stage_tour = stage_tour;
        self.problem = None;
        cx.notify();
    }

    /// The tour moved: the keyboard goes back to Next.
    pub fn moved(&mut self, cx: &mut Context<Self>) {
        self.button = None;
        self.menu = None;
        cx.notify();
    }

    /// F1 opened the guide: the menu and the shortcuts card close.
    pub fn close_cards(&mut self, cx: &mut Context<Self>) {
        self.button = None;
        self.menu = None;
        self.shortcuts = false;
        cx.notify();
    }

    /// Reset failed: the menu stays open and says so.
    pub fn reset_failed(&mut self, cx: &mut Context<Self>) {
        self.menu = Some(0);
        self.problem = Some(Text::GuideResetFailed);
        cx.notify();
    }

    fn layer(&self, cx: &App) -> Option<Layer> {
        if self.over_editor {
            return self.bardo.read(cx).tour_step(false).map(Layer::Tour);
        }
        let missed_open = self.missed.read(cx).is_open();
        if missed_open {
            // The missed posts come first; any tour but theirs waits behind
            // them.
            return self.bardo.read(cx).tour_step(true).map(Layer::Tour);
        }
        let bardo = self.bardo.read(cx);
        if let Some(step) = bardo.tour_step(false) {
            return Some(Layer::Tour(step));
        }
        if self.shortcuts {
            return Some(Layer::Shortcuts);
        }
        if let Some(row) = self.menu {
            return Some(Layer::Menu(row));
        }
        bardo.tour_offer().then_some(Layer::Offer)
    }

    fn menu_items(&self, cx: &App) -> Vec<MenuItem> {
        let bardo = self.bardo.read(cx);
        // The welcome tour, and the screen's own; the other screens' tours
        // start from their screens and their guide pages.
        let mut items = vec![MenuItem::UserGuide, MenuItem::Tour(TourId::Welcome)];
        items.extend(self.screen_tour.map(|tour| MenuItem::ScreenTour(tour.tour)));
        items.extend(self.stage_tour.map(|tour| MenuItem::StageTour(tour.tour)));
        if bardo.resumable_tour().is_some() {
            items.push(MenuItem::Resume);
        }
        items.extend([MenuItem::Shortcuts, MenuItem::Reset]);
        items
    }

    fn pick(&mut self, item: MenuItem, cx: &mut Context<Self>) {
        self.menu = None;
        match item {
            MenuItem::UserGuide => cx.emit(GuideEvent::OpenGuide),
            MenuItem::Tour(tour) | MenuItem::ScreenTour(tour) | MenuItem::StageTour(tour) => {
                cx.emit(GuideEvent::Start(tour))
            }
            MenuItem::Resume => cx.emit(GuideEvent::Resume),
            MenuItem::Shortcuts => self.shortcuts = true,
            MenuItem::Reset => cx.emit(GuideEvent::Reset),
        }
        cx.notify();
    }

    fn act(&mut self, act: Act, cx: &mut Context<Self>) {
        match act {
            Act::Send(event) => cx.emit(event),
            Act::CloseShortcuts => self.shortcuts = false,
        }
        cx.notify();
    }

    /// The buttons of the card on screen, in Tab order, and the one Enter
    /// presses when Tab was not used.
    fn buttons(layer: Layer) -> (Vec<(Text, Kind, Act)>, usize) {
        match layer {
            Layer::Tour(step) => {
                let mut buttons = vec![(Text::TourSkip, Kind::Ghost, Act::Send(GuideEvent::Skip))];
                if let Some(guide) = step.guide {
                    buttons.push((
                        Text::TourLearnMore,
                        Kind::Ghost,
                        Act::Send(GuideEvent::LearnMore(guide)),
                    ));
                }
                if !step.is_first() {
                    buttons.push((Text::TourBack, Kind::Outline, Act::Send(GuideEvent::Back)));
                }
                let next = if step.is_last() {
                    Text::TourFinish
                } else {
                    Text::TourNext
                };
                buttons.push((next, Kind::Primary, Act::Send(GuideEvent::Next)));
                let last = buttons.len() - 1;
                (buttons, last)
            }
            Layer::Offer => (
                vec![
                    (
                        Text::TourOfferNever,
                        Kind::Ghost,
                        Act::Send(GuideEvent::Decline { never: true }),
                    ),
                    (
                        Text::TourOfferNotNow,
                        Kind::Outline,
                        Act::Send(GuideEvent::Decline { never: false }),
                    ),
                    (
                        Text::TourOfferStart,
                        Kind::Primary,
                        Act::Send(GuideEvent::Start(TourId::Welcome)),
                    ),
                ],
                2,
            ),
            Layer::Shortcuts => (
                vec![(Text::ShortcutsClose, Kind::Primary, Act::CloseShortcuts)],
                0,
            ),
            Layer::Menu(_) => (Vec::new(), 0),
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(layer) = self.layer(cx) else {
            return;
        };
        let stroke = &event.keystroke;
        let modifiers = stroke.modifiers;
        if modifiers.control || modifiers.alt || modifiers.platform {
            return;
        }
        let key = stroke.key.as_str();
        if let Layer::Menu(row) = layer {
            let count = self.menu_items(cx).len();
            match key {
                "down" => self.menu = Some((row + 1) % count),
                "tab" if !modifiers.shift => self.menu = Some((row + 1) % count),
                "up" => self.menu = Some((row + count - 1) % count),
                "tab" => self.menu = Some((row + count - 1) % count),
                "enter" | "space" => {
                    let item = self.menu_items(cx)[row.min(count - 1)];
                    self.pick(item, cx);
                }
                "escape" => self.menu = None,
                _ => return,
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        let (buttons, default) = Self::buttons(layer);
        let focused = self.button.unwrap_or(default).min(buttons.len() - 1);
        match (layer, key) {
            (Layer::Tour(_), "right") => cx.emit(GuideEvent::Next),
            (Layer::Tour(step), "left") if !step.is_first() => cx.emit(GuideEvent::Back),
            (Layer::Tour(_), "escape") => cx.emit(GuideEvent::Close),
            (Layer::Offer, "escape") => cx.emit(GuideEvent::Decline { never: false }),
            (Layer::Shortcuts, "escape") => self.shortcuts = false,
            (_, "enter" | "space") => self.act(buttons[focused].2, cx),
            (_, "tab") => {
                let count = buttons.len();
                self.button = Some(if modifiers.shift {
                    (focused + count - 1) % count
                } else {
                    (focused + 1) % count
                });
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn button_row(&self, layer: Layer, cx: &mut Context<Self>) -> AnyElement {
        let (buttons, default) = Self::buttons(layer);
        let focused = self.button.unwrap_or(default);
        let t = look(cx).tokens;
        let bardo = self.bardo.read(cx);
        let mut lead = Vec::new();
        let mut trail = Vec::new();
        for (ix, (text, kind, act)) in buttons.into_iter().enumerate() {
            let mut button = Button::new(ElementId::Name(format!("guide-button-{ix}").into()))
                .small()
                .label(tr(bardo, text));
            button = match kind {
                Kind::Primary => button.primary(),
                Kind::Outline => button.outline(),
                Kind::Ghost => button.ghost(),
            };
            let button = button.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.act(act, cx);
            }));
            // The keyboard's button wears the focus ring; the others keep
            // the same room so nothing moves when Tab does.
            let framed = div()
                .p(px(1.))
                .rounded(t.radius + px(3.))
                .border_2()
                .border_color(if ix == focused {
                    t.focus
                } else {
                    gpui_kit::transparent_black()
                })
                .child(button);
            if kind == Kind::Ghost {
                lead.push(framed.into_any_element());
            } else {
                trail.push(framed.into_any_element());
            }
        }
        h_flex()
            .gap_2()
            .items_center()
            .justify_between()
            .child(h_flex().gap_1().children(lead))
            .child(h_flex().gap_1().children(trail))
            .into_any_element()
    }

    /// A card's frame: the surface, an outline that holds on the scrim,
    /// and the keyboard.
    fn frame(&self, width: f32, cx: &mut Context<Self>) -> gpui_kit::Stateful<gpui_kit::Div> {
        let t = look(cx).tokens;
        v_flex()
            .id("guide-card")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .w(px(width))
            .p_4()
            .gap_2()
            .bg(t.surface)
            .text_color(t.text)
            .border(t.border_width)
            .border_color(t.border_strong)
            .rounded(t.radius_lg)
            .shadow_lg()
    }

    fn tour_card(&self, step: TourStepView, cx: &mut Context<Self>) -> AnyElement {
        let t = look(cx).tokens;
        let bardo = self.bardo.read(cx);
        let count = bardo.text_with(
            Text::TourStepCount,
            &[
                ("n", &step.number.to_string()),
                ("count", &step.count.to_string()),
            ],
        );
        let header = h_flex()
            .justify_between()
            .text_xs()
            .text_color(t.text2)
            .child(SharedString::from(count))
            .child(tr(bardo, Text::TourEscHint));
        let title = div().text_lg().font_semibold().child(tr(bardo, step.title));
        let body = div()
            .text_sm()
            .text_color(t.text2)
            .child(tr(bardo, step.body));
        let buttons = self.button_row(Layer::Tour(step), cx);
        self.frame(CARD_WIDTH, cx)
            .child(header)
            .child(title)
            .child(body)
            .child(div().pt_1().child(buttons))
            .into_any_element()
    }

    fn offer_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = look(cx).tokens;
        let bardo = self.bardo.read(cx);
        let title = kit::title(tr(bardo, Text::TourOfferTitle));
        let body = div()
            .text_sm()
            .text_color(t.text2)
            .child(tr(bardo, Text::TourOfferBody));
        let buttons = self.button_row(Layer::Offer, cx);
        self.frame(OFFER_WIDTH, cx)
            .border_color(t.accent_edge)
            .gap_3()
            .child(title)
            .child(body)
            .child(buttons)
            .into_any_element()
    }

    fn shortcuts_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let buttons = self.button_row(Layer::Shortcuts, cx);
        let frame = self.frame(SHORTCUTS_WIDTH, cx);
        let t = look(cx).tokens;
        let mono = gpui_kit::component::ActiveTheme::theme(&**cx)
            .mono_font_family
            .clone();
        let bardo = self.bardo.read(cx);
        let groups = SHORTCUTS.iter().map(|group| {
            let rows = group.shortcuts.iter().map(|shortcut| {
                let keys = shortcut.keys.iter().map(|key| {
                    div()
                        .px_1p5()
                        .rounded(t.radius)
                        .border(t.border_width)
                        .border_b_2()
                        .border_color(t.border_strong)
                        .bg(t.sunken)
                        .text_xs()
                        .font_family(mono.clone())
                        .child(SharedString::new_static(key))
                });
                h_flex()
                    .gap_3()
                    .py_0p5()
                    .items_center()
                    .child(h_flex().w(px(170.)).flex_none().gap_1().children(keys))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .child(tr(bardo, shortcut.action)),
                    )
            });
            v_flex()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .font_semibold()
                        .text_color(t.text2)
                        .child(tr(bardo, group.name)),
                )
                .children(rows)
        });
        let title = kit::title(tr(bardo, Text::ShortcutsTitle));
        frame
            .gap_3()
            .child(title)
            .child(
                v_flex()
                    .id("guide-shortcuts")
                    .max_h(px(480.))
                    .overflow_y_scroll()
                    .gap_3()
                    .children(groups),
            )
            .child(h_flex().justify_end().child(buttons))
            .into_any_element()
    }

    fn menu_card(&self, row: usize, cx: &mut Context<Self>) -> AnyElement {
        let t = look(cx).tokens;
        let mono = gpui_kit::component::ActiveTheme::theme(&**cx)
            .mono_font_family
            .clone();
        let items = self.menu_items(cx);
        let bardo = self.bardo.read(cx);
        let rows: Vec<AnyElement> = items
            .iter()
            .enumerate()
            .map(|(ix, item)| {
                let item = *item;
                let (label, new) = match item {
                    MenuItem::UserGuide => (tr(bardo, Text::GuideUserGuide), false),
                    MenuItem::Tour(tour) => {
                        (tr(bardo, Text::TourName(tour)), bardo.tour_is_new(tour))
                    }
                    MenuItem::ScreenTour(_) => (
                        tr(bardo, Text::TourThisScreen),
                        self.screen_tour.is_some_and(|tour| tour.new),
                    ),
                    MenuItem::StageTour(_) => (
                        tr(bardo, Text::TourThisStage),
                        self.stage_tour.is_some_and(|tour| tour.new),
                    ),
                    MenuItem::Resume => (tr(bardo, Text::GuideResumeTour), false),
                    MenuItem::Shortcuts => (tr(bardo, Text::GuideShortcuts), false),
                    MenuItem::Reset => (tr(bardo, Text::GuideResetTours), false),
                };
                // The row's key, as the shortcuts list draws keys.
                let key = match item {
                    MenuItem::UserGuide => Some("F1"),
                    MenuItem::ScreenTour(_) if self.stage_tour.is_none() => Some("Shift+F1"),
                    MenuItem::StageTour(_) => Some("Shift+F1"),
                    _ => None,
                };
                let key = key.map(|key| {
                    div()
                        .px_1p5()
                        .rounded(t.radius)
                        .border(t.border_width)
                        .border_b_2()
                        .border_color(t.border_strong)
                        .bg(t.sunken)
                        .text_xs()
                        .text_color(t.text2)
                        .font_family(mono.clone())
                        .child(key)
                });
                h_flex()
                    .id(("guide-menu", ix))
                    .h(px(32.))
                    .px_2()
                    .gap_2()
                    .items_center()
                    .justify_between()
                    .rounded(t.radius)
                    .cursor_pointer()
                    .text_sm()
                    .when(ix == row, |row| row.bg(t.hover))
                    .hover(|row| row.bg(t.hover))
                    .child(label)
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .when(new, |side| {
                                side.child(kit::status(
                                    kit::Tone::Accent,
                                    tr(bardo, Text::TourNew),
                                    cx,
                                ))
                            })
                            .children(key),
                    )
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.pick(item, cx);
                    }))
                    .into_any_element()
            })
            .collect();
        let problem = self
            .problem
            .map(|problem| kit::notice(kit::Tone::Danger, tr(bardo, problem), cx));
        v_flex()
            .id("guide-menu")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .w(px(MENU_WIDTH))
            .p_1()
            .gap_0p5()
            .bg(t.raised)
            .text_color(t.text)
            .border(t.border_width)
            .border_color(t.border_strong)
            .rounded(t.radius_lg)
            .shadow_lg()
            .children(rows)
            .children(problem.map(|problem| div().p_2().child(problem)))
            .into_any_element()
    }

    /// What dims and rings, in the current theme.
    fn ring(cx: &App) -> Ring {
        let look = look(cx);
        let high_contrast = look.theme.family() == ThemeFamily::HighContrast;
        Ring {
            color: look.tokens.focus,
            width: px(if high_contrast { 3. } else { 2. }),
            radius: look.tokens.radius,
        }
    }

    fn layer_element(&self, layer: Layer, cx: &mut Context<Self>) -> AnyElement {
        let this = cx.entity().downgrade();
        let scrim = look(cx).tokens.scrim;
        match layer {
            Layer::Tour(step) => {
                let bardo = self.bardo.clone();
                let resolve: crate::tour::Resolve = Rc::new(move |drawn, cx| {
                    bardo.read(cx).tour_spot(drawn).unwrap_or(Spot::Center)
                });
                let number = step.number;
                Spotlight::new(resolve, self.tour_card(step, cx), Rc::clone(&self.motion))
                    .side(step.side)
                    .dim(scrim)
                    .ring(Self::ring(cx))
                    .block()
                    .on_skip(move |_, cx| {
                        let _ = this.update(cx, |_, cx| cx.emit(GuideEvent::StepOver(number)));
                    })
                    .into_any_element()
            }
            Layer::Shortcuts => {
                let center: crate::tour::Resolve = Rc::new(|_, _| Spot::Center);
                Spotlight::new(center, self.shortcuts_card(cx), Rc::default())
                    .dim(scrim)
                    .block()
                    .on_outside(move |_, cx| {
                        let _ = this.update(cx, |this, cx| {
                            this.shortcuts = false;
                            cx.notify();
                        });
                    })
                    .into_any_element()
            }
            Layer::Menu(row) => {
                let guide = TourAnchor::NavPlace(Destination::Guide);
                let resolve: crate::tour::Resolve = Rc::new(move |drawn, _| {
                    if drawn(guide) {
                        Spot::Lit(guide)
                    } else {
                        Spot::Center
                    }
                });
                Spotlight::new(resolve, self.menu_card(row, cx), Rc::default())
                    .block()
                    .on_outside(move |_, cx| {
                        let _ = this.update(cx, |this, cx| {
                            this.menu = None;
                            cx.notify();
                        });
                    })
                    .into_any_element()
            }
            Layer::Offer => {
                let center: crate::tour::Resolve = Rc::new(|_, _| Spot::Center);
                Spotlight::new(center, self.offer_card(cx), Rc::default()).into_any_element()
            }
        }
    }
}

impl Render for Guide {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let layer = self.layer(cx);
        let kind = |layer: Option<Layer>| layer.map(|layer| std::mem::discriminant(&layer));
        let changed = kind(layer) != kind(self.shown)
            || matches!((layer, self.shown), (Some(Layer::Tour(a)), Some(Layer::Tour(b))) if a.tour != b.tour);
        if changed {
            match layer {
                // A card took over: the keyboard goes to it, and comes back
                // where it was once every card is gone.
                Some(_) => {
                    if self.restore.is_none() {
                        self.restore = window.focused(cx).filter(|focused| *focused != self.focus);
                    }
                    let focus = self.focus.clone();
                    window.on_next_frame(move |window, cx| window.focus(&focus, cx));
                }
                None => {
                    if let Some(restore) = self.restore.take() {
                        let card = self.focus.clone();
                        window.on_next_frame(move |window, cx| {
                            // Unless what closed the card moved the keyboard
                            // on itself (the menu's "User guide" focuses the
                            // guide's search box).
                            if card.is_focused(window) || window.focused(cx).is_none() {
                                window.focus(&restore, cx);
                            }
                        });
                    }
                }
            }
            if !matches!(layer, Some(Layer::Tour(_))) {
                *self.motion.borrow_mut() = Motion::default();
            }
        }
        self.shown = layer;
        let root = div().absolute().inset_0();
        match layer {
            None => root.into_any_element(),
            Some(layer) => root
                .child(deferred(self.layer_element(layer, cx)).with_priority(PRIORITY))
                .into_any_element(),
        }
    }
}

#[cfg(test)]
mod tests {
    use bardo_app::Guide;
    use bardo_app::bardo_domain::UiLanguage;

    use super::refs;

    #[test]
    fn every_info_opens_a_section_in_every_language() {
        for language in UiLanguage::ALL {
            let guide = Guide::load(language);
            for at in refs::ALL {
                let page = guide
                    .page(at.page)
                    .unwrap_or_else(|| panic!("no page {} in {language:?}", at.page));
                assert!(
                    page.section(at.section).is_some(),
                    "no section {}#{} in {language:?}",
                    at.page,
                    at.section
                );
            }
        }
    }
}
