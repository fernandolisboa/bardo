//! The Workspace layout (direction B of spike #52): a sidebar grouped by
//! pillar, with jobs, costs, the guide and settings pinned at its foot;
//! each screen's header on top, the project stages as a stepper under it;
//! a grid of cards (or a feed) with the inspector in a right column, or a
//! list beside the record it opens.

use std::rc::Rc;

use bardo_app::{Destination, Side, Stage, StageState, TourAnchor};
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, ElementId, Hsla, SharedString, div, hsla, px, relative};

use crate::appearance::look;
use crate::icons;
use crate::kit;
use crate::parts::{
    Collection, CollectionKind, Figure, Header, Inspector, NavItem, Navigation, OnPick,
    ScreenParts, Sections, StageItem, Stages, Tile,
};
use crate::tour::Anchored as _;

use super::Room;

const SIDEBAR_WIDTH: f32 = 216.;
/// The sidebar as a rail of icons, in a narrow window.
const RAIL_WIDTH: f32 = 56.;
/// The narrowest window with the whole sidebar: below it, the sidebar is
/// a rail of icons, each place's name in its tooltip.
const RAIL_BELOW: f32 = 960.;
/// A list of records beside the one open.
const LIST_WIDTH: f32 = 300.;
const INSPECTOR_WIDTH: f32 = 340.;
const ASIDE_WIDTH: f32 = 300.;
/// The narrowest screen with the side columns at full width (the screen
/// beside the whole sidebar in a 960-pixel window): below it the list and
/// the inspector narrow, and the aside goes under the content.
const COMPACT_BELOW: f32 = 744.;
const COMPACT_LIST_WIDTH: f32 = 240.;
const COMPACT_INSPECTOR_WIDTH: f32 = 300.;
/// The longest a page's text runs.
const PAGE_WIDTH: f32 = 920.;
/// The narrowest a screen's title column gets, with the way here above
/// it (a pillar, "Projects › channel").
const TITLE_MIN_WIDTH: f32 = 320.;
/// The narrowest a project stage gets before the stepper scrolls.
const STEP_MIN_WIDTH: f32 = 132.;
/// The most cards per row of a grid.
const GRID_COLUMNS: u16 = 3;
/// The narrowest a card gets before the grid drops a column.
const CARD_MIN_WIDTH: f32 = 160.;
/// The space between cards, and around the main column's content.
const GRID_GAP: f32 = 12.;
const MAIN_PADDING: f32 = 16.;

/// How the layout fits the room it has (issue #153).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Fit {
    /// The sidebar as a rail of icons.
    rail: bool,
    /// The jobs panel over the screen rather than beside it.
    jobs_over: bool,
    /// The side columns narrowed, the aside under the content.
    compact: bool,
    /// The screen's width.
    screen: f32,
}

impl Fit {
    fn of(room: Room) -> Self {
        let rail = room.window < RAIL_BELOW;
        let nav = if rail { RAIL_WIDTH } else { SIDEBAR_WIDTH };
        let screen = room.screen(nav);
        Self {
            rail,
            jobs_over: room.jobs_over(nav),
            compact: screen < COMPACT_BELOW,
            screen,
        }
    }

    fn list(self) -> f32 {
        if self.compact {
            COMPACT_LIST_WIDTH
        } else {
            LIST_WIDTH
        }
    }

    fn inspector(self) -> f32 {
        if self.compact {
            COMPACT_INSPECTOR_WIDTH
        } else {
            INSPECTOR_WIDTH
        }
    }

    /// Cards per row of a grid in the main column, beside the inspector
    /// when there is one: as many as keep each card readable.
    fn grid_columns(self, inspector: bool) -> u16 {
        let main = self.screen - 2. * MAIN_PADDING - if inspector { self.inspector() } else { 0. };
        let fit = ((main + GRID_GAP) / (CARD_MIN_WIDTH + GRID_GAP)).floor();
        (fit.max(1.) as u16).min(GRID_COLUMNS)
    }
}

pub(super) fn shell(
    navigation: Navigation,
    screen: AnyElement,
    jobs: Option<AnyElement>,
    cx: &App,
) -> AnyElement {
    let t = &look(cx).tokens;
    let fit = Fit::of(super::room(cx));
    let (area, beside) = super::screen_area(screen, jobs, fit.jobs_over);
    h_flex()
        .size_full()
        .items_start()
        .bg(t.app)
        .text_color(t.text)
        .child(sidebar(navigation, fit.rail, cx))
        .child(area)
        .children(beside)
        .into_any_element()
}

/// The places grouped by pillar, with the pinned ones at the foot; as a
/// rail of icons when `rail`, each name in its icon's tooltip and the
/// pillars apart by a rule.
fn sidebar(nav: Navigation, rail: bool, cx: &App) -> AnyElement {
    let look = look(cx);
    let t = look.tokens;
    let pick = nav.on_pick.clone();
    // On the rail, `trailing` goes in the tooltip and `badge` over the icon.
    let row = |item: &NavItem,
               selected: bool,
               trailing: Option<AnyElement>,
               tip: Option<SharedString>,
               badge: Option<AnyElement>| {
        let place = item.place;
        let pick = Rc::clone(&pick);
        let icon = Icon::new(icons::destination(place)).size(px(16.));
        let row = nav_row(("nav", place as usize), selected, cx);
        let row = if rail {
            let tip = tip.unwrap_or_else(|| item.label.clone());
            row.relative()
                .justify_center()
                .child(icon)
                .children(badge.map(|badge| div().absolute().top_0p5().right_0p5().child(badge)))
                .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
        } else {
            row.child(icon)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(item.label.clone()),
                )
                .children(trailing)
        };
        row.on_click(move |_, window, cx| pick(place, window, cx))
    };

    let groups = nav.groups.iter().enumerate().map(|(ix, group)| {
        v_flex()
            .gap_0p5()
            .map(|column| {
                if !rail {
                    column.child(
                        div()
                            .px_2()
                            .pt_3()
                            .pb_1()
                            .text_xs()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .text_color(t.text2)
                            .child(group.label.to_uppercase()),
                    )
                } else if ix == 0 {
                    column.pt_2()
                } else {
                    column
                        .mt_2()
                        .pt_2()
                        .border_t(t.border_width)
                        .border_color(t.border)
                }
            })
            .children(group.places.iter().map(|item| {
                row(item, item.place == nav.current, None, None, None).tour_anchor(
                    TourAnchor::NavPlace(item.place),
                    Side::Right,
                    Some(&nav.scroll),
                )
            }))
            .tour_anchor(
                TourAnchor::NavGroup(group.pillar),
                Side::Right,
                Some(&nav.scroll),
            )
    });

    let pinned = nav.pinned.iter().map(|item| match item.place {
        Destination::Jobs => {
            let count = (nav.jobs > 0).then(|| {
                div()
                    .px_1p5()
                    .min_w(px(20.))
                    .text_center()
                    .rounded(t.radius)
                    .bg(t.sunken)
                    .border(t.border_width)
                    .border_color(t.border)
                    .text_xs()
                    .child(nav.jobs.to_string())
                    .into_any_element()
            });
            let (count, badge) = if rail { (None, count) } else { (count, None) };
            let tip = (nav.jobs > 0).then(|| nav.jobs_line.clone());
            row(item, nav.jobs_open, count, tip, badge)
                .tour_anchor(TourAnchor::NavPlace(item.place), Side::Right, None)
                .into_any_element()
        }
        Destination::Costs => {
            // On the rail: "Costs · $48.85 · 106% of this month's budgets".
            let tip = [
                Some(item.label.clone()),
                nav.spent.clone(),
                nav.budgets.as_ref().map(|budgets| budgets.line.clone()),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ");
            let spent = nav.spent.clone().map(|spent| {
                div()
                    .flex_none()
                    .text_xs()
                    .font_family(cx.theme().mono_font_family.clone())
                    .child(spent)
                    .into_any_element()
            });
            let meter = nav.budgets.as_ref().map(|budgets| {
                let ink = budgets.tone.ink(cx);
                let ink = if budgets.tone == kit::Tone::Neutral {
                    t.text2
                } else {
                    ink
                };
                v_flex()
                    .px_2()
                    .pb_1()
                    .gap_1()
                    .child(
                        div()
                            .h(px(4.))
                            .w_full()
                            .rounded(t.radius)
                            .bg(t.sunken)
                            .child(
                                div()
                                    .h_full()
                                    .rounded(t.radius)
                                    .bg(ink)
                                    .w(relative(budgets.percent.min(100) as f32 / 100.)),
                            ),
                    )
                    .when(!rail, |meter| {
                        meter.child(
                            div()
                                .text_xs()
                                .text_color(t.text2)
                                .child(budgets.line.clone()),
                        )
                    })
            });
            v_flex()
                .gap_1()
                .child(row(
                    item,
                    nav.current == Destination::Costs,
                    spent,
                    Some(tip.into()),
                    None,
                ))
                .children(meter)
                .tour_anchor(TourAnchor::NavPlace(item.place), Side::Right, None)
                .into_any_element()
        }
        _ => {
            // The Guide opens a menu over the screen, which stays; the
            // menu's "User guide" opens the Guide screen.
            let selected = match item.place {
                Destination::Guide => nav.guide_open || nav.current == Destination::Guide,
                place => nav.current == place,
            };
            row(item, selected, None, None, None)
                .tour_anchor(TourAnchor::NavPlace(item.place), Side::Right, None)
                .into_any_element()
        }
    });

    v_flex()
        .w(px(if rail { RAIL_WIDTH } else { SIDEBAR_WIDTH }))
        .h_full()
        .flex_none()
        .bg(t.surface)
        .border_r(t.border_width)
        .border_color(t.border)
        .child(
            v_flex()
                .id("nav-groups")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&nav.scroll)
                .px_2()
                .children(groups),
        )
        .child(
            v_flex()
                .p_2()
                .gap_0p5()
                .border_t(t.border_width)
                .border_color(t.border)
                .children(pinned),
        )
        .into_any_element()
}

/// A row of the sidebar; the current place is tinted.
fn nav_row(
    id: impl Into<ElementId>,
    selected: bool,
    cx: &App,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let t = look(cx).tokens;
    h_flex()
        .id(id)
        .h(px(32.))
        .px_2()
        .gap_2()
        .rounded(t.radius)
        .cursor_pointer()
        .text_sm()
        .font_weight(gpui_kit::FontWeight::MEDIUM)
        .map(|row| {
            if selected {
                row.bg(t.selected).text_color(t.accent_text)
            } else {
                row.text_color(t.text2)
                    .hover(|row| row.bg(t.hover).text_color(t.text))
            }
        })
}

pub(super) fn screen(parts: ScreenParts, cx: &App) -> AnyElement {
    let ScreenParts {
        header,
        stages,
        toolbar,
        notices,
        collection,
        inspector,
        mut content,
        aside,
        summary,
        sections,
        scroll,
    } = parts;
    let t = &look(cx).tokens;
    let fit = Fit::of(super::room(cx));
    let staged = stages.is_some();
    // The figures as cards, the toolbar, then the sections as tabs over
    // the one shown.
    let mut lead: Vec<AnyElement> = Vec::new();
    if !summary.is_empty() {
        lead.push(
            kit::anchor_in(TourAnchor::Summary, figures(summary, cx), scroll.as_ref())
                .into_any_element(),
        );
    }
    // In the scroll like the figures, so a tour scrolled down to the
    // inspector brings it back into view.
    lead.extend(toolbar.map(|toolbar| {
        kit::anchor_in(TourAnchor::Toolbar, toolbar, scroll.as_ref()).into_any_element()
    }));
    if let Some(sections) = sections {
        let (tabs, shown) = section_tabs(sections);
        lead.push(tabs);
        content.extend(shown);
    }

    // In a narrow screen the aside goes under the content, in its column.
    let (aside, aside_under) = if fit.compact && !aside.is_empty() {
        (Vec::new(), Some(aside_under(aside, cx)))
    } else {
        (aside, None)
    };

    let body = match collection {
        Some(collection) if collection.kind == CollectionKind::List => h_flex()
            .flex_1()
            .min_h_0()
            .items_start()
            .child(list_column(collection, fit.list(), cx))
            .child(
                v_flex()
                    .id("screen-record")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .when_some(scroll, |record, scroll| record.track_scroll(&scroll))
                    .child(
                        v_flex()
                            .max_w(px(PAGE_WIDTH))
                            .p_6()
                            .gap_4()
                            .children(lead)
                            .children(notices)
                            .children(inspector.map(|inspector| inspector_body(inspector, cx)))
                            .children(content)
                            .children(aside_under),
                    ),
            )
            .when(!aside.is_empty(), |row| {
                row.child(
                    v_flex()
                        .id("screen-aside")
                        .w(px(ASIDE_WIDTH))
                        .h_full()
                        .flex_none()
                        .overflow_y_scroll()
                        .p_4()
                        .gap_3()
                        .bg(t.surface)
                        .border_l(t.border_width)
                        .border_color(t.border)
                        .children(aside),
                )
            })
            .into_any_element(),
        collection @ Some(_) => {
            main_with_inspector(fit, lead, notices, collection, content, inspector, cx)
        }
        None if inspector.is_some() => {
            main_with_inspector(fit, lead, notices, None, content, inspector, cx)
        }
        None => v_flex()
            .id("screen-page")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .when_some(scroll, |page, scroll| page.track_scroll(&scroll))
            .px_6()
            .pb_6()
            .when(staged, |page| page.pt_4())
            .gap_4()
            .children(lead)
            .children(notices)
            .child(
                h_flex()
                    .gap_4()
                    .items_start()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .max_w(px(PAGE_WIDTH))
                            .gap_4()
                            .children(content)
                            .children(aside_under),
                    )
                    .when(!aside.is_empty(), |row| {
                        row.child(
                            v_flex()
                                .w(px(ASIDE_WIDTH))
                                .flex_none()
                                .gap_3()
                                .children(aside),
                        )
                    }),
            )
            .tour_anchor(TourAnchor::Content, Side::Right, None)
            .into_any_element(),
    };

    v_flex()
        .size_full()
        .bg(t.app)
        .text_color(t.text)
        .child(screen_header(header, cx))
        .children(stages.map(|stages| stepper(stages, cx)))
        .child(body)
        .into_any_element()
}

fn screen_header(header: Header, cx: &App) -> AnyElement {
    let t = &look(cx).tokens;
    let mut trail = Vec::new();
    for (ix, crumb) in header.trail.into_iter().enumerate() {
        if ix > 0 {
            trail.push(div().child("›").into_any_element());
        }
        trail.push(crumb);
    }
    // The title's column never gets narrower than TITLE_MIN_WIDTH: in a
    // narrow window the actions drop below it instead (issue #142).
    h_flex()
        .flex_none()
        .flex_wrap()
        .px_6()
        .pt_4()
        .pb_3()
        .gap_x_4()
        .gap_y_2()
        .items_center()
        .child(
            v_flex()
                .flex_1()
                .min_w(px(TITLE_MIN_WIDTH))
                .gap_0p5()
                .when(!trail.is_empty(), |column| {
                    column.child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .text_xs()
                            .text_color(t.text2)
                            .children(trail),
                    )
                })
                // The ⓘ and tour buttons follow the title while both fit,
                // and drop below it otherwise: they never cut the title.
                .child(
                    h_flex()
                        .flex_wrap()
                        .gap_x_2()
                        .gap_y_1()
                        .items_center()
                        .child(
                            div()
                                .text_xl()
                                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                .min_w_0()
                                .child(header.title),
                        )
                        .children(header.info),
                )
                .children(
                    header
                        .meta
                        .map(|meta| div().text_xs().text_color(t.text2).child(meta)),
                ),
        )
        .child(
            h_flex()
                .flex_none()
                .ml_auto()
                .gap_2()
                .items_center()
                .children(header.actions),
        )
        .tour_anchor(TourAnchor::Header, Side::Below, None)
        .into_any_element()
}

fn stepper(stages: Stages, cx: &App) -> AnyElement {
    let t = &look(cx).tokens;
    let steps = stages
        .items
        .into_iter()
        .map(|item| step(item, stages.current, Rc::clone(&stages.on_pick), cx));
    h_flex()
        .flex_none()
        .flex_wrap()
        .px_6()
        .gap_1()
        .border_b(t.border_width)
        .border_color(t.border)
        .children(steps)
        .tour_anchor(TourAnchor::Stages, Side::Below, None)
        .into_any_element()
}

fn step(item: StageItem, current: Stage, on_pick: OnPick<Stage>, cx: &App) -> AnyElement {
    let t = &look(cx).tokens;
    let stage = item.status.stage;
    let state = item.status.state;
    let on = stage == current;
    let locked = state == StageState::Locked;
    let (icon, ink, tint): (Icon, Hsla, Hsla) = match state {
        _ if on => (Icon::new(icons::stage(stage)), t.on_accent, t.accent),
        StageState::Done => (Icon::new(IconName::Check), t.success, t.success_bg),
        StageState::Working => (Icon::new(IconName::LoaderCircle), t.info, t.info_bg),
        StageState::Attention => (Icon::new(IconName::TriangleAlert), t.warning, t.warning_bg),
        StageState::Partial | StageState::Open => {
            (Icon::new(icons::stage(stage)), t.text2, t.sunken)
        }
        StageState::Locked => (
            Icon::new(gpui_kit::assets::IconName::Lock),
            t.text2,
            t.sunken,
        ),
    };
    let round = if t.radius == px(0.) { px(0.) } else { px(999.) };
    // Seven stages share the width: each takes its part, and its line
    // wraps rather than pushing the last ones out of sight; below a
    // readable width the steps go on to a second row (issue #73).
    h_flex()
        .id(("stage", stage as usize))
        .flex_1()
        .min_w(px(STEP_MIN_WIDTH))
        .items_start()
        .gap_2()
        .px_2()
        .pt_2()
        .pb(px(10.))
        .border_b_2()
        .border_color(if on {
            t.accent
        } else {
            gpui_kit::transparent_black()
        })
        .when(!locked, |step| {
            step.cursor_pointer()
                .hover(|step| step.bg(t.hover))
                .on_click(move |_, window, cx| on_pick(stage, window, cx))
        })
        .child(
            div()
                .size(px(28.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(round)
                .bg(tint)
                .when(locked || state == StageState::Open, |dot| {
                    dot.border(t.border_width).border_color(t.border)
                })
                .child(icon.size(px(14.)).text_color(ink)),
        )
        .child(
            v_flex()
                .min_w_0()
                .child(
                    div()
                        .text_sm()
                        .truncate()
                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                        .text_color(if locked { t.text2 } else { t.text })
                        .child(item.name),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(match state {
                            StageState::Attention => t.warning,
                            _ => t.text2,
                        })
                        .child(item.note),
                ),
        )
        .into_any_element()
}

/// The aside's parts under the content, in a narrow screen.
fn aside_under(aside: Vec<AnyElement>, cx: &App) -> AnyElement {
    let t = &look(cx).tokens;
    v_flex()
        .id("screen-aside")
        .pt_4()
        .gap_3()
        .border_t(t.border_width)
        .border_color(t.border)
        .children(aside)
        .into_any_element()
}

/// The main column, with the inspector on the right when there is one.
fn main_with_inspector(
    fit: Fit,
    lead: Vec<AnyElement>,
    notices: Vec<AnyElement>,
    collection: Option<Collection>,
    content: Vec<AnyElement>,
    inspector: Option<Inspector>,
    cx: &App,
) -> AnyElement {
    let t = &look(cx).tokens;
    let columns = fit.grid_columns(inspector.is_some());
    let items = collection.map(|collection| collection_body(collection, columns, cx));
    h_flex()
        .flex_1()
        .min_h_0()
        .items_start()
        .child(
            v_flex()
                .id("screen-main")
                .flex_1()
                .min_w_0()
                .h_full()
                .overflow_y_scroll()
                .p_4()
                .gap_3()
                .children(lead)
                .children(notices)
                .children(items)
                .children(content),
        )
        .children(inspector.map(|inspector| {
            let footer = inspector.footer;
            let scroll = inspector.scroll;
            v_flex()
                .w(px(fit.inspector()))
                .h_full()
                .flex_none()
                .bg(t.surface)
                .border_l(t.border_width)
                .border_color(t.border)
                .child(
                    v_flex()
                        .id("inspector")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .when_some(scroll, |inspector, scroll| inspector.track_scroll(&scroll))
                        .p_4()
                        .gap_3()
                        .children(inspector.title)
                        .children(inspector.body),
                )
                .children(footer.map(|footer| {
                    div()
                        .flex_none()
                        .px_4()
                        .py_2()
                        .border_t(t.border_width)
                        .border_color(t.border)
                        .child(footer)
                }))
                .tour_anchor(TourAnchor::Inspector, Side::Left, None)
        }))
        .into_any_element()
}

fn inspector_body(inspector: Inspector, _cx: &App) -> AnyElement {
    v_flex()
        .gap_4()
        .children(inspector.title)
        .children(inspector.body)
        .children(inspector.footer)
        .tour_anchor(TourAnchor::Inspector, Side::Below, None)
        .into_any_element()
}

/// A grid or a feed, with its controls above.
fn collection_body(collection: Collection, columns: u16, cx: &App) -> AnyElement {
    let empty = collection.is_empty();
    let controls = (!collection.controls.is_empty()).then(|| {
        h_flex()
            .gap_2()
            .flex_wrap()
            .items_center()
            .children(collection.controls)
    });
    let items: AnyElement = if empty {
        div().children(collection.empty).into_any_element()
    } else if collection.kind == CollectionKind::Grid {
        div()
            .grid()
            .grid_cols(columns)
            .gap_3()
            .children(collection.tiles.into_iter().map(|tile| card(tile, cx)))
            .into_any_element()
    } else {
        v_flex()
            .gap_3()
            .children(collection.tiles.into_iter().map(|tile| row(tile, cx)))
            .children(collection.cards)
            .into_any_element()
    };
    // Its full height in the scrolling column: shrunk to the column's,
    // the cards' pictures collapse (issue #153).
    super::take_keys(v_flex().id(collection.id), collection.keys)
        .flex_none()
        .gap_3()
        .children(controls)
        .child(items)
        .tour_anchor(TourAnchor::Collection, Side::Below, None)
        .into_any_element()
}

/// A list of records in a column of its own, beside the open one.
fn list_column(collection: Collection, width: f32, cx: &App) -> AnyElement {
    let t = &look(cx).tokens;
    let empty = collection.is_empty();
    let scroll = collection.keys.as_ref().map(|keys| keys.scroll.clone());
    let rows = collection.tiles.into_iter().flat_map(|mut tile| {
        let heading = tile.group.take().map(|group| {
            div()
                .px_2()
                .pt_3()
                .pb_1()
                .text_xs()
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .text_color(t.text2)
                .child(group.to_uppercase())
                .into_any_element()
        });
        heading.into_iter().chain(std::iter::once(row(tile, cx)))
    });
    v_flex()
        .w(px(width))
        .h_full()
        .flex_none()
        .bg(t.surface)
        .border_r(t.border_width)
        .border_color(t.border)
        .when(!collection.controls.is_empty(), |column| {
            column.child(
                h_flex()
                    .p_3()
                    .gap_2()
                    .flex_wrap()
                    .items_center()
                    .children(collection.controls),
            )
        })
        .child(
            super::take_keys(v_flex().id(collection.id), collection.keys)
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .when_some(scroll, |list, scroll| list.track_scroll(&scroll))
                .p_2()
                .gap_1()
                .when(empty, |list| {
                    list.child(div().p_1().children(collection.empty))
                })
                .children(rows)
                .children(collection.cards),
        )
        .tour_anchor(TourAnchor::Collection, Side::Right, None)
        .into_any_element()
}

/// A tile as a card: its picture with the number, time and state over
/// it, then its marks and two lines of text.
fn card(tile: Tile, cx: &App) -> AnyElement {
    let t = &look(cx).tokens;
    let on_click = tile.on_click;
    let edge = if tile.selected {
        t.accent
    } else if tile.failed {
        t.danger
    } else if tile.attention {
        t.accent_edge
    } else {
        t.frame
    };
    // Over a picture, in every theme.
    let scrim = hsla(0., 0., 0., 0.62);
    let overlay = |text| {
        div()
            .absolute()
            .px_1p5()
            .rounded(t.radius)
            .bg(scrim)
            .text_color(gpui_kit::white())
            .text_xs()
            .font_family(cx.theme().mono_font_family.clone())
            .child(text)
    };
    v_flex()
        .id(tile.id)
        .min_w_0()
        .bg(t.surface)
        .border(if tile.selected {
            px(2.)
        } else {
            t.border_width
        })
        .border_color(edge)
        .rounded(t.radius_lg)
        .overflow_hidden()
        .cursor_pointer()
        .child(
            div()
                .relative()
                .w_full()
                .aspect_ratio(16. / 9.)
                .bg(t.sunken)
                .overflow_hidden()
                .children(
                    tile.picture
                        .map(|picture| div().absolute().inset_0().child(picture)),
                )
                .children(tile.number.map(|number| overlay(number).top_2().left_2()))
                .children(tile.time.map(|time| overlay(time).bottom_2().left_2()))
                .children(
                    tile.status
                        .map(|status| div().absolute().top_2().right_2().child(status)),
                ),
        )
        .child(
            h_flex()
                .p_2()
                .gap_2()
                .items_start()
                .child(h_flex().flex_none().gap_1().children(tile.marks))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .children(tile.title.map(|title| {
                            div()
                                .text_sm()
                                .font_weight(gpui_kit::FontWeight::MEDIUM)
                                .child(title)
                        }))
                        .children(
                            tile.text.map(|text| {
                                div().text_sm().line_clamp(2).text_ellipsis().child(text)
                            }),
                        ),
                ),
        )
        .on_click(move |event, window, cx| on_click(event, window, cx))
        .into_any_element()
}

/// A tile as a row: title and state, then its line.
fn row(tile: Tile, cx: &App) -> AnyElement {
    let t = &look(cx).tokens;
    let on_click = tile.on_click;
    kit::list_row(tile.id, tile.selected, cx)
        .child(
            h_flex()
                .gap_2()
                .justify_between()
                .items_center()
                .child(
                    h_flex()
                        .gap_2()
                        .min_w_0()
                        .children(
                            tile.number
                                .map(|number| div().text_sm().text_color(t.text2).child(number)),
                        )
                        .children(tile.title.map(|title| div().min_w_0().child(title))),
                )
                .child(
                    h_flex()
                        .flex_none()
                        .gap_1()
                        .children(tile.marks)
                        .children(tile.status),
                ),
        )
        .children(
            tile.text
                .map(|text| div().text_xs().text_color(t.text2).child(text)),
        )
        .children(
            tile.time
                .map(|time| div().text_xs().text_color(t.text2).child(time)),
        )
        .on_click(move |event, window, cx| on_click(event, window, cx))
        .into_any_element()
}

/// The page's figures as a row of cards.
fn figures(summary: Vec<Figure>, cx: &App) -> AnyElement {
    let t = look(cx).tokens;
    let mono = cx.theme().mono_font_family.clone();
    h_flex()
        .gap_3()
        .flex_wrap()
        .items_stretch()
        .children(summary.into_iter().map(|figure| {
            kit::card(cx)
                .flex_1()
                .min_w(px(220.))
                .p_4()
                .gap_1()
                .when_some(figure.tone, |card, tone| card.border_color(tone.ink(cx)))
                .child(div().text_sm().text_color(t.text2).child(figure.label))
                .child(
                    h_flex()
                        .gap_2()
                        .items_baseline()
                        .child(
                            div()
                                .text_2xl()
                                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                .font_family(mono.clone())
                                .child(figure.value),
                        )
                        .children(figure.beside.map(|beside| {
                            div()
                                .font_weight(gpui_kit::FontWeight::MEDIUM)
                                .child(beside)
                        })),
                )
                .children(figure.line)
        }))
        .into_any_element()
}

/// The sections as tabs, and the one shown.
fn section_tabs(sections: Sections) -> (AnyElement, Option<AnyElement>) {
    let Sections {
        id,
        items,
        selected,
        on_pick,
    } = sections;
    let mut bar = TabBar::new(id).underline().selected_index(selected);
    let mut shown = None;
    for (ix, section) in items.into_iter().enumerate() {
        bar = bar.child(Tab::new().label(section.title));
        if ix == selected {
            shown = Some(section.body);
        }
    }
    let bar = bar
        .on_click(move |ix: &usize, window, cx| on_pick(*ix, window, cx))
        .into_any_element();
    (bar, shown)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fit(window: f32, jobs_open: bool) -> Fit {
        Fit::of(Room { window, jobs_open })
    }

    #[test]
    fn nothing_changes_from_the_old_minimum_up() {
        for window in [960., 1260., 1920.] {
            let fit = fit(window, false);
            assert!(!fit.rail && !fit.compact, "{window}");
            assert_eq!(fit.list(), LIST_WIDTH);
            assert_eq!(fit.inspector(), INSPECTOR_WIDTH);
        }
        assert_eq!(fit(1260., false).grid_columns(true), GRID_COLUMNS);
    }

    #[test]
    fn a_laptop_snap_half_gets_the_rail_and_narrow_columns() {
        for window in [640., 683., 768., 853.] {
            let fit = fit(window, false);
            assert!(fit.rail, "{window}");
            assert_eq!(fit.screen, window - RAIL_WIDTH);
        }
        let half = fit(640., false);
        assert!(half.compact);
        assert_eq!(half.list(), COMPACT_LIST_WIDTH);
        assert_eq!(half.inspector(), COMPACT_INSPECTOR_WIDTH);
        // The record beside the list keeps a readable width.
        assert!(half.screen - half.list() >= 320.);
    }

    #[test]
    fn the_grid_drops_columns_rather_than_squeeze_its_cards() {
        for (window, columns) in [(1260., 3), (960., 2), (640., 1)] {
            let fit = fit(window, false);
            assert_eq!(fit.grid_columns(true), columns, "{window}");
            let main = fit.screen - 2. * MAIN_PADDING - fit.inspector();
            let card = (main - GRID_GAP * f32::from(columns - 1)) / f32::from(columns);
            assert!(card >= CARD_MIN_WIDTH, "{window}: {card}");
        }
    }

    #[test]
    fn the_jobs_panel_lies_over_the_screen_in_a_narrow_window() {
        assert!(!fit(1260., true).jobs_over);
        let narrow = fit(960., true);
        assert!(narrow.jobs_over);
        assert_eq!(narrow.screen, 960. - SIDEBAR_WIDTH);
    }
}
