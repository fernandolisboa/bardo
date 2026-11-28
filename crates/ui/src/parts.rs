//! The parts every screen is made of, apart from where they go (issue
//! #60). A screen builds its parts (they hold its behavior: the clicks,
//! the state shown, the explanations) and hands them to the current
//! layout's arrangement in [`crate::layout`], which only places them. A
//! second layout reuses the parts as they are and places them its own way.
//!
//! What a part shows comes from `bardo_app` (the navigation, the project
//! stages, the month's spend); nothing here decides anything.

use std::rc::Rc;

use bardo_app::{
    Bardo, Control, Destination, GuidePlace, GuideRef, LibraryTab, NavEntry, NavSection, Stage,
    StageStatus, Step, Text, TourAnchor,
};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, ElementId, FocusHandle, ScrollHandle, SharedString, Window,
};

use crate::shell::tr;

/// What a click runs.
pub type OnClick = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// What picking one of `T` (a place, a stage) runs.
pub type OnPick<T> = Rc<dyn Fn(T, &mut Window, &mut App)>;

/// What a key runs.
pub type OnKey = Rc<dyn Fn(&mut Window, &mut App)>;

/// A screen, in parts. Every part but the header is optional.
pub struct ScreenParts {
    pub header: Header,
    /// The project stages, on the projects screen.
    pub stages: Option<Stages>,
    /// Actions over the collection or the page: filters, the primary action.
    pub toolbar: Option<AnyElement>,
    /// Messages about the whole screen: saved, failed, a budget question.
    pub notices: Vec<AnyElement>,
    pub collection: Option<Collection>,
    /// The selected item's properties, or the form that drives the
    /// collection.
    pub inspector: Option<Inspector>,
    /// What the screen shows when it is a page rather than a collection
    /// (a script, settings), or below the collection.
    pub content: Vec<AnyElement>,
    /// Summaries beside the content (spend by channel).
    pub aside: Vec<AnyElement>,
    /// The page's figures (spent, budgets in alert), above its content.
    pub summary: Vec<Figure>,
    /// Parts of the content shown one at a time or all together, as the
    /// layout prefers; after `content`.
    pub sections: Option<Sections>,
    /// The scroll that holds the content, when the screen moves it itself
    /// (the guide goes to a section); the layout tracks it on the scroll
    /// it draws a page's content in, or a list's beside it. Grids and
    /// forms do not take it.
    pub scroll: Option<ScrollHandle>,
}

impl ScreenParts {
    pub fn new(header: Header) -> Self {
        Self {
            header,
            stages: None,
            toolbar: None,
            notices: Vec::new(),
            collection: None,
            inspector: None,
            content: Vec::new(),
            aside: Vec::new(),
            summary: Vec::new(),
            sections: None,
            scroll: None,
        }
    }
}

/// Where the screen sits and what it shows.
pub struct Header {
    /// The way here, before the title: "Ideas", "Videos › channel".
    pub trail: Vec<AnyElement>,
    /// The title; may be a switcher.
    pub title: AnyElement,
    /// One line under the title.
    pub meta: Option<SharedString>,
    /// The ⓘ that explains the screen.
    pub info: Option<AnyElement>,
    /// What sits on the other side: the month, "New".
    pub actions: Vec<AnyElement>,
    /// A row of settings under the title, the full width of the header:
    /// an open video's summary and the cap's warnings (#330, #366).
    pub bar: Option<AnyElement>,
    /// The places shown under the same entry, as tabs under the title:
    /// Research and Themes, Channels and Accounts, the global library's.
    pub tabs: Option<PlaceTabs>,
}

/// Places under one entry of the navigation, as tabs.
pub struct PlaceTabs {
    pub items: Vec<SharedString>,
    pub selected: usize,
    pub on_pick: OnPick<usize>,
    /// What a tour lights them as.
    pub anchor: Option<TourAnchor>,
}

/// The place the shell shows, which may be the global library showing
/// one of its tabs' screens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShownPlace(pub Destination);

impl gpui_kit::Global for ShownPlace {}

impl Header {
    pub fn new(title: impl IntoElement) -> Self {
        Self {
            trail: Vec::new(),
            title: title.into_any_element(),
            meta: None,
            info: None,
            actions: Vec::new(),
            bar: None,
            tabs: None,
        }
    }

    /// A place's own header: its name, with the entry it sits under and
    /// that entry's places as tabs when it has several. Shown in the global
    /// library, a library screen's header is the library's, with its tabs.
    pub fn place(bardo: &Bardo, place: Destination, cx: &App) -> Self {
        let shown = cx.try_global::<ShownPlace>().map(|shown| shown.0);
        if shown == Some(Destination::Library)
            && let Some(tab) = LibraryTab::of(place)
        {
            let mut header = Self::new(tr(bardo, Text::DestinationName(Destination::Library)));
            header.meta = Some(tr(bardo, Text::LibraryHint));
            header.tabs = Some(PlaceTabs {
                items: LibraryTab::ALL
                    .iter()
                    .map(|tab| tr(bardo, Text::DestinationName(tab.place())))
                    .collect(),
                selected: LibraryTab::ALL.iter().position(|t| *t == tab).unwrap_or(0),
                on_pick: Rc::new(|ix, window, cx| {
                    if let Some(tab) = LibraryTab::ALL.get(ix) {
                        crate::guide::go(GuidePlace::Library(*tab), window, cx);
                    }
                }),
                anchor: Some(TourAnchor::Control(Control::LibraryTabs)),
            });
            return header;
        }
        let mut header = Self::new(tr(bardo, Text::DestinationName(place)));
        if let Some(entry) = place.entry().filter(|entry| entry.has_tabs()) {
            let places = entry.places();
            header.trail = vec![tr(bardo, Text::NavEntryName(entry)).into_any_element()];
            header.tabs = Some(PlaceTabs {
                items: places
                    .iter()
                    .map(|place| tr(bardo, Text::DestinationName(*place)))
                    .collect(),
                selected: places.iter().position(|p| *p == place).unwrap_or(0),
                on_pick: Rc::new(move |ix, window, cx| {
                    if let Some(place) = places.get(ix) {
                        crate::guide::go(GuidePlace::Screen(*place), window, cx);
                    }
                }),
                anchor: None,
            });
        }
        header
    }
}

/// The project's stages, the one on screen, and what picking one does.
pub struct Stages {
    pub items: Vec<StageItem>,
    pub current: Stage,
    pub on_pick: OnPick<Stage>,
}

/// A stage with its name and status line.
pub struct StageItem {
    pub status: StageStatus,
    pub name: SharedString,
    pub note: SharedString,
}

impl Stages {
    pub fn new(
        bardo: &Bardo,
        stages: &[StageStatus],
        current: Stage,
        on_pick: impl Fn(Stage, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            items: stages
                .iter()
                .map(|status| StageItem {
                    status: *status,
                    name: tr(bardo, Text::StageName(status.stage)),
                    note: SharedString::from(bardo.stage_note(status.note)),
                })
                .collect(),
            current,
            on_pick: Rc::new(on_pick),
        }
    }
}

/// How a collection's items are best read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectionKind {
    /// Pictures first: scenes and their clips.
    Grid,
    /// Records picked to edit: channels, personas, template versions.
    List,
    /// Results read in full: ranked niches, theme ideas.
    Feed,
}

/// The items a screen holds.
pub struct Collection {
    pub kind: CollectionKind,
    pub id: ElementId,
    /// Above the items: a filter, kind buttons, a count.
    pub controls: Vec<AnyElement>,
    /// `Grid` and `List` items.
    pub tiles: Vec<Tile>,
    /// `Feed` items, drawn by the screen.
    pub cards: Vec<AnyElement>,
    /// What shows instead of items when there are none, or why they could
    /// not load.
    pub empty: Option<AnyElement>,
    /// What a table of the items calls its columns.
    pub headings: Headings,
    /// ↑/↓ and Enter over the items, when they take the keyboard.
    pub keys: Option<CollectionKeys>,
}

impl Collection {
    pub fn new(kind: CollectionKind, id: impl Into<ElementId>) -> Self {
        Self {
            kind,
            id: id.into(),
            controls: Vec::new(),
            tiles: Vec::new(),
            cards: Vec::new(),
            empty: None,
            headings: Headings::default(),
            keys: None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty() && self.cards.is_empty()
    }
}

/// One item of a collection, as facts an arrangement lays out as a card,
/// a row or a table line.
pub struct Tile {
    pub id: ElementId,
    /// The name of the group this tile starts, in a list grouped under
    /// headings (the guide's contents by pillar). Each heading is a row of
    /// its own, so the list's scroll counts it among the items.
    pub group: Option<SharedString>,
    pub selected: bool,
    /// The item's picture, filling the box it is given.
    pub picture: Option<AnyElement>,
    /// A position: the scene number.
    pub number: Option<SharedString>,
    /// A time span: the scene's start and end.
    pub time: Option<SharedString>,
    pub title: Option<SharedString>,
    /// A line or two about it.
    pub text: Option<SharedString>,
    /// Its state, as a chip.
    pub status: Option<AnyElement>,
    /// Small state marks: image and clip icons.
    pub marks: Vec<AnyElement>,
    /// Whether the state needs the user (drawn edged).
    pub attention: bool,
    /// Whether the state is a failure (drawn edged in danger).
    pub failed: bool,
    /// A second text, quieter: the scene's image prompt.
    pub detail: Option<SharedString>,
    /// Properties a table shows a column each for: the image and clip
    /// states, the model, the cost. Every tile of a collection lists the
    /// same ones, in the same order.
    pub facts: Vec<Fact>,
    pub on_click: OnClick,
}

impl Tile {
    pub fn new(id: impl Into<ElementId>, on_click: OnClick) -> Self {
        Self {
            id: id.into(),
            group: None,
            selected: false,
            picture: None,
            number: None,
            time: None,
            title: None,
            text: None,
            status: None,
            marks: Vec::new(),
            attention: false,
            failed: false,
            detail: None,
            facts: Vec::new(),
            on_click,
        }
    }
}

/// A property of a tile, under its column's name.
pub struct Fact {
    pub label: SharedString,
    pub value: AnyElement,
    /// Figures align right.
    pub numeric: bool,
}

/// The names of a table's columns for a tile's parts; a part without a
/// name keeps its column unnamed.
#[derive(Default)]
pub struct Headings {
    pub picture: Option<SharedString>,
    pub time: Option<SharedString>,
    pub text: Option<SharedString>,
    pub detail: Option<SharedString>,
}

/// The keyboard over a collection: what ↑/↓ and Enter do, and the focus
/// that takes the keys (the screen keeps it, so it outlives a render).
pub struct CollectionKeys {
    pub focus: FocusHandle,
    pub on_step: OnPick<Step>,
    /// Accepts what the selected item waits on; `None` when it waits on
    /// nothing.
    pub on_enter: Option<OnKey>,
    /// What the keys do, behind an ⓘ.
    pub hint: Option<KeysHint>,
    /// Scrolls the items, when a layout gives them a scroll of their own,
    /// so the screen can bring the selected one into view.
    pub scroll: ScrollHandle,
}

/// What the keys over a collection do, as its ⓘ says it: the text, the
/// guide's "More in the guide" and the section that opens.
#[derive(Clone)]
pub struct KeysHint {
    pub text: SharedString,
    pub more: SharedString,
    pub section: GuideRef,
}

/// A figure of the page: "Spent in October: $40.70".
pub struct Figure {
    pub label: SharedString,
    pub value: SharedString,
    /// Beside the value: "of 2 in alert".
    pub beside: Option<SharedString>,
    /// Under it: what it is made of, a link to act on it.
    pub line: Option<AnyElement>,
    /// Edges its card when it needs the user.
    pub tone: Option<crate::kit::Tone>,
    /// The figure as a short phrase or chips, for a one-line summary;
    /// without one, the value and the label.
    pub brief: Option<AnyElement>,
}

impl Figure {
    pub fn new(label: impl Into<SharedString>, value: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            beside: None,
            line: None,
            tone: None,
            brief: None,
        }
    }
}

/// Parts of a page under their names: the budgets and the rate table.
pub struct Sections {
    pub id: ElementId,
    pub items: Vec<Section>,
    /// The one shown when only one is.
    pub selected: usize,
    pub on_pick: OnPick<usize>,
}

pub struct Section {
    pub title: SharedString,
    pub body: AnyElement,
}

/// The selected item's properties.
pub struct Inspector {
    /// The item's name line, with what sits beside it.
    pub title: Option<AnyElement>,
    pub body: Vec<AnyElement>,
    /// Which part of `body` is the item's picture, or its current and new
    /// one side by side; a layout may set it apart.
    pub media: Option<usize>,
    /// Pinned at the bottom: provenance behind "Generation details".
    pub footer: Option<AnyElement>,
    /// The scroll that holds `body` (but not a picture set apart), when the
    /// screen tags controls in it for a tour: the layout tracks it where it
    /// scrolls the inspector on its own, beside a grid or under a table.
    pub scroll: Option<ScrollHandle>,
}

impl Inspector {
    pub fn new(body: Vec<AnyElement>) -> Self {
        Self {
            title: None,
            body,
            media: None,
            footer: None,
            scroll: None,
        }
    }
}

/// The app-wide navigation: the project's places, the global library and
/// the pinned ones,
/// where the user is, the month's spend and the job count, and what
/// picking a place does.
pub struct Navigation {
    pub groups: Vec<NavGroup>,
    pub pinned: Vec<NavItem>,
    pub current: Destination,
    /// Whether the jobs panel is open beside the screen.
    pub jobs_open: bool,
    /// Whether the Guide's menu is open.
    pub guide_open: bool,
    /// The places' scroll, when a layout scrolls them; the shell keeps it.
    pub scroll: ScrollHandle,
    /// Jobs queued or running.
    pub jobs: usize,
    /// This month's spend, formatted.
    pub spent: Option<SharedString>,
    /// The jobs as a line: "2 jobs running".
    pub jobs_line: SharedString,
    /// This month's spend as a line: "$9.59 in October 2026".
    pub spent_line: Option<SharedString>,
    /// The budgets' use, when any is set: percent and its line.
    pub budgets: Option<BudgetMeter>,
    pub on_pick: OnPick<Destination>,
}

/// A section and its entries.
pub struct NavGroup {
    pub section: NavSection,
    pub label: SharedString,
    pub places: Vec<NavItem>,
}

/// An entry of the navigation.
pub struct NavItem {
    /// Where a click goes: the entry's place, or the tab last shown under
    /// it.
    pub place: Destination,
    /// The entry's first place: its icon, its id and what tours light.
    pub home: Destination,
    /// Every place the entry shows; it reads as current on any of them.
    pub places: &'static [Destination],
    pub label: SharedString,
}

impl NavItem {
    fn of(bardo: &Bardo, place: Destination) -> Self {
        Self {
            place,
            home: place,
            places: std::slice::from_ref(
                Destination::ALL
                    .iter()
                    .find(|p| **p == place)
                    .expect("every place is listed"),
            ),
            label: tr(bardo, Text::DestinationName(place)),
        }
    }

    fn entry(bardo: &Bardo, entry: NavEntry, last: Destination) -> Self {
        let places = entry.places();
        Self {
            place: if places.contains(&last) {
                last
            } else {
                places[0]
            },
            home: places[0],
            places,
            label: tr(bardo, Text::NavEntryName(entry)),
        }
    }

    /// Whether `current` is one of its places.
    pub fn holds(&self, current: Destination) -> bool {
        self.places.contains(&current)
    }
}

impl Navigation {
    /// The places as `bardo_app` groups them, with nothing counted yet;
    /// `last` is the place last shown under an entry with tabs.
    pub fn new(
        bardo: &Bardo,
        current: Destination,
        last: impl Fn(NavEntry) -> Destination,
        on_pick: impl Fn(Destination, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            groups: Destination::SECTIONS
                .iter()
                .map(|(section, entries)| NavGroup {
                    section: *section,
                    label: tr(bardo, Text::NavSectionName(*section)),
                    places: entries
                        .iter()
                        .map(|entry| NavItem::entry(bardo, *entry, last(*entry)))
                        .collect(),
                })
                .collect(),
            pinned: Destination::PINNED
                .iter()
                .map(|place| NavItem::of(bardo, *place))
                .collect(),
            current,
            jobs_open: false,
            guide_open: false,
            scroll: ScrollHandle::new(),
            jobs: 0,
            spent: None,
            jobs_line: tr(bardo, Text::StatusNoJobs),
            spent_line: None,
            budgets: None,
            on_pick: Rc::new(on_pick),
        }
    }
}

/// How this month's budgets stand together.
pub struct BudgetMeter {
    pub percent: u64,
    pub tone: crate::kit::Tone,
    pub line: SharedString,
}
