//! The Guide screen (issue #106): the user guide inside the app. The table
//! of contents (or the search results) is the collection, the page is the
//! content, and "On this page" is the aside, so each layout places it as
//! it places any other screen. The pages are `bardo_app`'s [`Guide`],
//! read from `docs/guide/`; this screen only draws them and follows their
//! links.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use bardo_app::bardo_domain::{TourId, UiLanguage};
use bardo_app::{
    Bardo, Destination, GuideHit, GuideLink, GuidePage, GuidePlace, SettingsTab, Step, Text,
};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState, MoveDown, MoveUp};
use gpui_kit::component::text::{TextView, TextViewStyle};
use gpui_kit::component::{IconName, Sizable as _, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Bounds, ClickEvent, ElementId, Entity, EventEmitter, FocusHandle, KeyBinding,
    Pixels, Point, ScrollHandle, SharedString, Subscription, Window, actions, canvas, div, point,
    px, rems,
};

use crate::appearance::look;
use crate::kit;
use crate::layout;
use crate::parts::{Collection, CollectionKeys, CollectionKind, Header, ScreenParts, Tile};
use crate::shell::tr;

actions!(guide, [OpenGuide]);

/// F1 opens the guide from any screen.
pub fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("f1", OpenGuide, None)]);
}

/// The page's Markdown is laid out in the background, section by section,
/// so a move to a section keeps aiming at it until its place has held for
/// `REVEAL_STEADY` frames, for at most `REVEAL_FRAMES` frames.
const REVEAL_FRAMES: u8 = 60;
const REVEAL_STEADY: u8 = 4;

/// What the guide asks of the window around it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuideScreenEvent {
    /// "Show me", or a tour link: start the tour.
    Tour(TourId),
    /// "Go to …", or a place link: open the place.
    Go(GuidePlace),
}

/// A search's results, for the query and language they answer.
struct Found {
    query: String,
    language: UiLanguage,
    hits: Rc<Vec<GuideHit>>,
}

/// A section to bring to the top of the page, or the page's top.
struct Reveal {
    section: Option<String>,
    frames: u8,
    /// Frames the target has held still.
    steady: u8,
    /// Where the reveal left the page last frame; any other offset means
    /// the user scrolled, and the reveal gives way.
    left_at: Option<Point<Pixels>>,
}

pub struct GuideScreen {
    bardo: Entity<Bardo>,
    /// The open page's id.
    page: String,
    search: Entity<InputState>,
    /// What the search box holds, as last read.
    query: String,
    /// The search result the keyboard is on.
    highlight: usize,
    /// Takes ↑/↓ and Enter over the contents.
    keys: FocusHandle,
    list_scroll: ScrollHandle,
    /// The scroll that holds the page.
    scroll: ScrollHandle,
    /// Where each section of the open page was drawn, by id.
    sections: Rc<RefCell<HashMap<String, Bounds<Pixels>>>>,
    reveal: Option<Reveal>,
    /// The language the search box's placeholder is in.
    language: Option<UiLanguage>,
    /// The last search's results, for its query and language.
    found: RefCell<Option<Found>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<GuideScreenEvent> for GuideScreen {}

impl GuideScreen {
    pub fn new(bardo: Entity<Bardo>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx));
        let subscriptions = vec![cx.subscribe_in(
            &search,
            window,
            |this, input, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    this.query = input.read(cx).value().to_string();
                    this.highlight = 0;
                    this.list_scroll.scroll_to_item(0);
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.pick(window, cx),
                _ => {}
            },
        )];
        let page = bardo.read(cx).guide().first().id.clone();
        Self {
            bardo,
            page,
            search,
            query: String::new(),
            highlight: 0,
            keys: cx.focus_handle(),
            list_scroll: ScrollHandle::new(),
            scroll: ScrollHandle::new(),
            sections: Rc::default(),
            reveal: None,
            language: None,
            found: RefCell::default(),
            _subscriptions: subscriptions,
        }
    }

    /// Opens `page`, at `section` or at its top. A page the guide does not
    /// have leaves the open one.
    pub fn open(&mut self, page: &str, section: Option<&str>, cx: &mut Context<Self>) {
        let Some(found) = self.bardo.read(cx).guide().page(page) else {
            tracing::warn!(page, "no such guide page");
            return;
        };
        let section = section.filter(|section| found.section(section).is_some());
        if self.page != page {
            self.sections.borrow_mut().clear();
        }
        self.page = page.to_owned();
        self.reveal = Some(Reveal {
            section: section.map(str::to_owned),
            frames: REVEAL_FRAMES,
            steady: 0,
            left_at: None,
        });
        cx.notify();
    }

    /// F1: the page for `place` (else its screen's, else the first), with
    /// the keyboard in the search box.
    pub fn open_at(
        &mut self,
        place: Option<GuidePlace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let page = self.bardo.read(cx).guide().page_at(place).id.clone();
        self.open(&page, None, cx);
        self.focus_search(window, cx);
    }

    pub fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search
            .update(cx, |search, cx| search.focus(window, cx));
    }

    /// The search's results, run once per query and language.
    fn hits(&self, cx: &App) -> Rc<Vec<GuideHit>> {
        if self.query.trim().is_empty() {
            return Rc::default();
        }
        let guide = self.bardo.read(cx).guide();
        let mut found = self.found.borrow_mut();
        if let Some(found) = &*found
            && found.query == self.query
            && found.language == guide.language()
        {
            return found.hits.clone();
        }
        let hits = Rc::new(guide.search(&self.query));
        *found = Some(Found {
            query: self.query.clone(),
            language: guide.language(),
            hits: hits.clone(),
        });
        hits
    }

    /// The pages in the contents' order.
    fn page_order(&self, cx: &App) -> Vec<String> {
        self.bardo
            .read(cx)
            .guide()
            .contents()
            .into_iter()
            .flat_map(|(_, pages)| pages)
            .map(|page| page.id.clone())
            .collect()
    }

    /// The contents' row of `page`, counting each group's heading row.
    fn contents_row(&self, page: &str, cx: &App) -> usize {
        let mut row = 0;
        for (_, pages) in self.bardo.read(cx).guide().contents() {
            row += 1;
            match pages.iter().position(|entry| entry.id == page) {
                Some(at) => return row + at,
                None => row += pages.len(),
            }
        }
        0
    }

    /// ↑/↓: through the search results while searching, else to the
    /// previous or next page.
    fn step(&mut self, step: Step, cx: &mut Context<Self>) {
        if self.query.trim().is_empty() {
            let order = self.page_order(cx);
            let at = order.iter().position(|id| *id == self.page).unwrap_or(0);
            let to = match step {
                Step::Previous => at.saturating_sub(1),
                Step::Next => (at + 1).min(order.len().saturating_sub(1)),
            };
            if to != at {
                let page = order[to].clone();
                self.open(&page, None, cx);
                self.list_scroll
                    .scroll_to_item(self.contents_row(&page, cx));
            }
            return;
        }
        let count = self.hits(cx).len();
        if count == 0 {
            return;
        }
        self.highlight = match step {
            Step::Previous => self.highlight.saturating_sub(1),
            Step::Next => (self.highlight + 1).min(count - 1),
        };
        self.list_scroll.scroll_to_item(self.highlight);
        cx.notify();
    }

    /// Enter: opens the highlighted search result.
    fn pick(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let hits = self.hits(cx);
        if let Some(hit) = hits.get(self.highlight.min(hits.len().saturating_sub(1))) {
            self.open(&hit.page, hit.section.as_deref(), cx);
        }
    }

    fn clear_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search
            .update(cx, |search, cx| search.set_value("", window, cx));
        self.query.clear();
        self.highlight = 0;
        cx.notify();
    }

    /// Follows a link clicked in a page.
    fn follow(&mut self, url: &str, cx: &mut Context<Self>) {
        match GuideLink::parse(url, &self.page) {
            Some(GuideLink::Page { page, section }) => self.open(&page, section.as_deref(), cx),
            Some(GuideLink::Tour(tour)) => cx.emit(GuideScreenEvent::Tour(tour)),
            Some(GuideLink::Go(place)) => cx.emit(GuideScreenEvent::Go(place)),
            Some(GuideLink::External(url)) => cx.open_url(&url),
            None => tracing::warn!(url, page = self.page, "a guide link leads nowhere"),
        }
    }

    /// Brings the section asked for to the top of the page, once its place
    /// is known.
    fn apply_reveal(&mut self, cx: &mut Context<Self>) {
        let Some(reveal) = &mut self.reveal else {
            return;
        };
        let current = self.scroll.offset();
        if reveal.left_at.is_some_and(|left_at| left_at != current) {
            self.reveal = None;
            return;
        }
        let target = match &reveal.section {
            None => Some(point(current.x, px(0.))),
            Some(section) => self.sections.borrow().get(section).map(|bounds| {
                let viewport = self.scroll.bounds();
                let max = self.scroll.max_offset();
                let y = current.y - (bounds.top() - viewport.top()) + px(8.);
                point(current.x, y.clamp(-max.y, px(0.)))
            }),
        };
        match target {
            Some(target) if target != current => {
                self.scroll.set_offset(target);
                reveal.steady = 0;
            }
            Some(_) => reveal.steady += 1,
            // Not laid out yet.
            None => {}
        }
        reveal.left_at = Some(target.unwrap_or(current));
        reveal.frames = reveal.frames.saturating_sub(1);
        if reveal.frames == 0 || reveal.steady >= REVEAL_STEADY {
            self.reveal = None;
        }
        cx.notify();
    }

    /// The contents by group, or the search results.
    fn collection(&self, page: &GuidePage, cx: &mut Context<Self>) -> Collection {
        let mut collection = Collection::new(CollectionKind::List, "guide-contents");
        let search = div()
            .w_full()
            .capture_action(cx.listener(|this, _: &MoveUp, _, cx| {
                this.step(Step::Previous, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &MoveDown, _, cx| {
                this.step(Step::Next, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &Escape, window, cx| {
                if !this.query.is_empty() {
                    this.clear_search(window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                Input::new(&self.search)
                    .small()
                    .cleanable(true)
                    .prefix(gpui_kit::component::Icon::new(IconName::Search).small()),
            );
        collection.controls = vec![search.into_any_element()];

        let this = cx.entity().downgrade();
        let on_step = {
            let this = this.clone();
            Rc::new(move |step: Step, _: &mut Window, cx: &mut App| {
                let _ = this.update(cx, |this, cx| this.step(step, cx));
            })
        };
        let on_enter = {
            let this = this.clone();
            Rc::new(move |window: &mut Window, cx: &mut App| {
                let _ = this.update(cx, |this, cx| this.pick(window, cx));
            })
        };
        let bardo = self.bardo.read(cx);
        collection.keys = Some(CollectionKeys {
            focus: self.keys.clone(),
            on_step,
            on_enter: Some(on_enter),
            hint: Some(tr(bardo, Text::GuideSearchKeys)),
            scroll: self.list_scroll.clone(),
        });

        if self.query.trim().is_empty() {
            for (group, pages) in bardo.guide().contents() {
                for (ix, entry) in pages.into_iter().enumerate() {
                    let id = entry.id.clone();
                    let mut tile = Tile::new(
                        ElementId::Name(format!("guide-page-{id}").into()),
                        Rc::new(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.open(&id, None, cx);
                        })),
                    );
                    tile.group = (ix == 0).then(|| tr(bardo, Text::GuideGroupName(group)));
                    tile.title = Some(SharedString::from(entry.title.clone()));
                    tile.selected = entry.id == page.id;
                    collection.tiles.push(tile);
                }
            }
            return collection;
        }

        let hits = self.hits(cx);
        if hits.is_empty() {
            collection.empty = Some(
                div()
                    .text_sm()
                    .text_color(look(cx).tokens.text2)
                    .child(SharedString::from(bardo.text_with(
                        Text::GuideSearchEmpty,
                        &[("query", self.query.trim())],
                    )))
                    .into_any_element(),
            );
        }
        let highlight = self.highlight.min(hits.len().saturating_sub(1));
        for (ix, hit) in hits.iter().enumerate() {
            let title = match &hit.section_title {
                Some(section) => format!("{} › {section}", hit.page_title),
                None => hit.page_title.clone(),
            };
            let (target, section) = (hit.page.clone(), hit.section.clone());
            let mut tile = Tile::new(
                ("guide-hit", ix),
                Rc::new(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.highlight = ix;
                    this.open(&target, section.as_deref(), cx);
                })),
            );
            tile.title = Some(SharedString::from(title));
            tile.text = Some(SharedString::from(hit.snippet.clone()));
            tile.selected = ix == highlight;
            collection.tiles.push(tile);
        }
        collection
    }

    /// One block of Markdown, following its links here.
    fn markdown(&self, id: String, markdown: &str, cx: &mut Context<Self>) -> TextView {
        let this = cx.entity().downgrade();
        TextView::markdown(ElementId::Name(id.into()), markdown.to_owned())
            .selectable(true)
            .style(TextViewStyle::default().paragraph_gap(rems(0.75)))
            .on_link_click(move |url, _, _, cx| {
                let url = url.to_string();
                let _ = this.update(cx, |this, cx| this.follow(&url, cx));
            })
    }

    /// The page: its introduction, then each section under its heading.
    fn content(&self, page: &GuidePage, cx: &mut Context<Self>) -> AnyElement {
        let language = self.bardo.read(cx).ui_language();
        let key = format!("guide-{language}-{}", page.id);
        let mut blocks: Vec<AnyElement> = Vec::new();
        if !page.intro.is_empty() {
            blocks.push(
                self.markdown(format!("{key}-intro"), &page.intro, cx)
                    .into_any_element(),
            );
        }
        for section in &page.sections {
            let sections = Rc::clone(&self.sections);
            let id = section.id.clone();
            // Records where the section was drawn, for links to it.
            let probe = canvas(
                move |bounds, _, _| {
                    sections.borrow_mut().insert(id.clone(), bounds);
                },
                |_, _, _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full();
            blocks.push(
                v_flex()
                    .relative()
                    .gap_2()
                    .child(kit::section_heading(SharedString::from(
                        section.title.clone(),
                    )))
                    .child(self.markdown(format!("{key}-{}", section.id), &section.body, cx))
                    .child(probe)
                    .into_any_element(),
            );
        }
        v_flex()
            .gap_6()
            .text_sm()
            .children(blocks)
            .into_any_element()
    }

    /// "On this page": the sections, the page's tour and its place.
    fn aside(&self, page: &GuidePage, cx: &mut Context<Self>) -> AnyElement {
        let t = look(cx).tokens;
        let bardo = self.bardo.read(cx);
        let links = page.sections.iter().map(|section| {
            let (page_id, id) = (page.id.clone(), section.id.clone());
            div()
                .id(ElementId::Name(format!("guide-toc-{id}").into()))
                .px_2()
                .py_1()
                .rounded(t.radius)
                .text_sm()
                .text_color(t.text2)
                .cursor_pointer()
                .hover(|row| row.bg(t.hover).text_color(t.text))
                .child(SharedString::from(section.title.clone()))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.open(&page_id, Some(&id), cx);
                }))
        });
        let show_me = page.tour.map(|tour| {
            Button::new("guide-show-me")
                .small()
                .primary()
                .icon(IconName::Eye)
                .label(tr(bardo, Text::GuideShowMe))
                .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                    cx.emit(GuideScreenEvent::Tour(tour));
                }))
        });
        let go_to = page.place.map(|place| {
            let label = bardo.text_with(Text::GuideGoTo, &[("place", &place_name(bardo, place))]);
            Button::new("guide-go-to")
                .small()
                .outline()
                .label(SharedString::from(label))
                .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                    cx.emit(GuideScreenEvent::Go(place));
                }))
        });
        v_flex()
            .gap_3()
            .child(
                v_flex()
                    .gap_0p5()
                    .child(
                        div()
                            .px_2()
                            .pb_1()
                            .text_xs()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .text_color(t.text2)
                            .child(tr(bardo, Text::GuideOnThisPage).to_uppercase()),
                    )
                    .children(links),
            )
            .when(show_me.is_some() || go_to.is_some(), |aside| {
                aside.child(
                    v_flex()
                        .gap_2()
                        .pt_3()
                        .border_t(t.border_width)
                        .border_color(t.border)
                        .children(show_me)
                        .children(go_to),
                )
            })
            .into_any_element()
    }
}

/// A place's name as the navigation says it: "Settings › API keys".
fn place_name(bardo: &Bardo, place: GuidePlace) -> String {
    let (screen, part) = match place {
        GuidePlace::Screen(place) => (place, None),
        GuidePlace::Stage(stage) => (Destination::Projects, Some(Text::StageName(stage))),
        GuidePlace::Settings(tab) => (
            Destination::Settings,
            Some(match tab {
                SettingsTab::Keys => Text::SettingsKeysTab,
                SettingsTab::Networks => Text::SettingsNetworksTab,
                SettingsTab::Appearance => Text::SettingsAppearanceTab,
                SettingsTab::Metrics => Text::MetricsSettingsTab,
            }),
        ),
    };
    let screen = bardo.text(Text::DestinationName(screen)).into_owned();
    match part {
        Some(part) => format!("{screen} › {}", bardo.text(part)),
        None => screen,
    }
}

impl Render for GuideScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let language = self.bardo.read(cx).ui_language();
        if self.language != Some(language) {
            self.language = Some(language);
            let placeholder = tr(self.bardo.read(cx), Text::GuideSearchPlaceholder);
            self.search.update(cx, |search, cx| {
                search.set_placeholder(placeholder, window, cx)
            });
        }
        if self.reveal.is_some() {
            let this = cx.entity().downgrade();
            window.on_next_frame(move |_, cx| {
                let _ = this.update(cx, |this, cx| this.apply_reveal(cx));
            });
        }
        let guide = self.bardo.read(cx).guide();
        let page = guide
            .page(&self.page)
            .unwrap_or_else(|| guide.first())
            .clone();
        let bardo = self.bardo.read(cx);
        let mut header = Header::new(SharedString::from(page.title.clone()));
        header.trail = vec![
            tr(bardo, Text::DestinationName(Destination::Guide)).into_any_element(),
            tr(bardo, Text::GuideGroupName(page.group)).into_any_element(),
        ];
        let mut parts = ScreenParts::new(header);
        parts.collection = Some(self.collection(&page, cx));
        parts.content = vec![self.content(&page, cx)];
        parts.aside = vec![self.aside(&page, cx)];
        parts.scroll = Some(self.scroll.clone());
        layout::screen(parts, cx)
    }
}
