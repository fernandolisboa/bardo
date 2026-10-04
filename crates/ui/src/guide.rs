//! The Guide (issue #105): the first-run offer, the help menu behind the
//! Guide place, the keyboard shortcuts, and the guided tour on screen. All
//! of it is drawn deferred over the content area, after everything else.
//! The tour's moves go to the shell as [`GuideEvent`]s, since some of them
//! open a place; what the tour is and where it stands is `bardo_app`'s.

use std::cell::RefCell;
use std::rc::Rc;

use bardo_app::bardo_domain::{ThemeFamily, TourId};
use bardo_app::{Bardo, Destination, SHORTCUTS, Side, Spot, Text, TourAnchor, TourStepView};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, ElementId, Entity, EventEmitter, FocusHandle, KeyDownEvent,
    SharedString, Window, deferred, div, px,
};

use crate::appearance::look;
use crate::kit;
use crate::missed::MissedPosts;
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
}

/// A row of the help menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuItem {
    Tour(TourId),
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
    shortcuts: bool,
    /// The card button the keyboard is on.
    button: Option<usize>,
    motion: Rc<RefCell<Motion>>,
    /// Why the last menu action failed.
    problem: Option<Text>,
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
            shortcuts: false,
            button: None,
            motion: Rc::default(),
            problem: None,
        }
    }

    pub fn is_menu_open(&self) -> bool {
        self.menu.is_some()
    }

    /// The Guide place was picked: opens or closes its menu.
    pub fn toggle_menu(&mut self, cx: &mut Context<Self>) {
        self.menu = match self.menu {
            Some(_) => None,
            None => Some(0),
        };
        self.problem = None;
        cx.notify();
    }

    /// The tour moved: the keyboard goes back to Next.
    pub fn moved(&mut self, cx: &mut Context<Self>) {
        self.button = None;
        self.menu = None;
        cx.notify();
    }

    /// Reset failed: the menu stays open and says so.
    pub fn reset_failed(&mut self, cx: &mut Context<Self>) {
        self.menu = Some(0);
        self.problem = Some(Text::GuideResetFailed);
        cx.notify();
    }

    fn layer(&self, cx: &App) -> Option<Layer> {
        let missed_open = self.missed.read(cx).is_open();
        if missed_open {
            // The missed posts come first; the tour waits behind them.
            return None;
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
        let mut items: Vec<MenuItem> = TourId::ALL.into_iter().map(MenuItem::Tour).collect();
        if bardo.resumable_tour().is_some() {
            items.push(MenuItem::Resume);
        }
        items.extend([MenuItem::Shortcuts, MenuItem::Reset]);
        items
    }

    fn pick(&mut self, item: MenuItem, cx: &mut Context<Self>) {
        self.menu = None;
        match item {
            MenuItem::Tour(tour) => cx.emit(GuideEvent::Start(tour)),
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
    fn buttons(&self, layer: Layer, cx: &App) -> (Vec<(Text, Kind, Act)>, usize) {
        match layer {
            Layer::Tour(step) => {
                let mut buttons = vec![(Text::TourSkip, Kind::Ghost, Act::Send(GuideEvent::Skip))];
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
            Layer::Menu(_) => {
                let _ = cx;
                (Vec::new(), 0)
            }
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
        let (buttons, default) = self.buttons(layer, cx);
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
        let (buttons, default) = self.buttons(layer, cx);
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
        let items = self.menu_items(cx);
        let bardo = self.bardo.read(cx);
        let rows: Vec<AnyElement> = items
            .iter()
            .enumerate()
            .map(|(ix, item)| {
                let item = *item;
                let (label, new) = match item {
                    MenuItem::Tour(tour) => {
                        (tr(bardo, Text::TourName(tour)), bardo.tour_is_new(tour))
                    }
                    MenuItem::Resume => (tr(bardo, Text::GuideResumeTour), false),
                    MenuItem::Shortcuts => (tr(bardo, Text::GuideShortcuts), false),
                    MenuItem::Reset => (tr(bardo, Text::GuideResetTours), false),
                };
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
                    .when(new, |row| {
                        row.child(kit::status(kit::Tone::Accent, tr(bardo, Text::TourNew), cx))
                    })
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
                Spotlight::new(center, self.offer_card(cx), Rc::default())
                    .side(Side::Open)
                    .into_any_element()
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
                        window.on_next_frame(move |window, cx| window.focus(&restore, cx));
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
