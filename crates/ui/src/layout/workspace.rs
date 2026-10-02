//! The Workspace layout (direction B of spike #52): a sidebar grouped by
//! pillar, with jobs, costs and settings pinned at its foot; each screen's
//! header on top, the project stages as a stepper under it; a grid of
//! cards (or a feed) with the inspector in a right column, or a list
//! beside the record it opens.

use std::rc::Rc;

use bardo_app::{Destination, Stage, StageState};
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, ElementId, Hsla, div, hsla, px, relative};

use crate::appearance::look;
use crate::icons;
use crate::kit;
use crate::parts::{
    Collection, CollectionKind, Header, Inspector, NavItem, Navigation, OnPick, ScreenParts,
    StageItem, Stages, Tile,
};

const SIDEBAR_WIDTH: f32 = 216.;
/// A list of records beside the one open.
const LIST_WIDTH: f32 = 300.;
const INSPECTOR_WIDTH: f32 = 340.;
const ASIDE_WIDTH: f32 = 300.;
/// The longest a page's text runs.
const PAGE_WIDTH: f32 = 920.;
/// The narrowest a project stage gets before the stepper scrolls.
const STEP_MIN_WIDTH: f32 = 132.;
/// Cards per row of a grid.
const GRID_COLUMNS: u16 = 3;

pub(super) fn shell(
    navigation: Navigation,
    screen: AnyElement,
    jobs: Option<AnyElement>,
    cx: &App,
) -> AnyElement {
    let t = &look(cx).tokens;
    h_flex()
        .size_full()
        .items_start()
        .bg(t.app)
        .text_color(t.text)
        .child(sidebar(navigation, cx))
        .child(div().flex_1().h_full().min_w_0().child(screen))
        .children(jobs)
        .into_any_element()
}

fn sidebar(nav: Navigation, cx: &App) -> AnyElement {
    let look = look(cx);
    let t = look.tokens;
    let pick = nav.on_pick.clone();
    let row = |item: &NavItem, selected: bool, trailing: Option<AnyElement>| {
        let place = item.place;
        let pick = Rc::clone(&pick);
        nav_row(("nav", place as usize), selected, cx)
            .child(Icon::new(icons::destination(place)).size(px(16.)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(item.label.clone()),
            )
            .children(trailing)
            .on_click(move |_, window, cx| pick(place, window, cx))
    };

    let groups = nav.groups.iter().map(|group| {
        v_flex()
            .gap_0p5()
            .child(
                div()
                    .px_2()
                    .pt_3()
                    .pb_1()
                    .text_xs()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .text_color(t.text2)
                    .child(group.label.to_uppercase()),
            )
            .children(
                group
                    .places
                    .iter()
                    .map(|item| row(item, item.place == nav.current, None)),
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
            row(item, nav.jobs_open, count).into_any_element()
        }
        Destination::Costs => {
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
                    .child(
                        div()
                            .text_xs()
                            .text_color(t.text2)
                            .child(budgets.line.clone()),
                    )
            });
            v_flex()
                .gap_1()
                .child(row(item, nav.current == Destination::Costs, spent))
                .children(meter)
                .into_any_element()
        }
        _ => row(item, nav.current == item.place, None).into_any_element(),
    });

    v_flex()
        .w(px(SIDEBAR_WIDTH))
        .h_full()
        .flex_none()
        .bg(t.surface)
        .border_r(t.border_width)
        .border_color(t.border)
        .child(
            h_flex()
                .px_4()
                .pt_4()
                .pb_2()
                .gap_2()
                .child(
                    div()
                        .size(px(18.))
                        .rounded(if t.radius == px(0.) { px(0.) } else { px(5.) })
                        .bg(t.accent),
                )
                .child(
                    div()
                        .text_lg()
                        .font_weight(gpui_kit::FontWeight::BOLD)
                        .child(nav.app_name.clone()),
                ),
        )
        .child(
            v_flex()
                .id("nav-groups")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
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
        content,
        aside,
    } = parts;
    let t = &look(cx).tokens;
    let staged = stages.is_some();

    let body = match collection {
        Some(collection) if collection.kind == CollectionKind::List => h_flex()
            .flex_1()
            .min_h_0()
            .items_start()
            .child(list_column(collection, cx))
            .child(
                v_flex()
                    .id("screen-record")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .child(
                        v_flex()
                            .max_w(px(PAGE_WIDTH))
                            .p_6()
                            .gap_4()
                            .children(toolbar)
                            .children(notices)
                            .children(inspector.map(|inspector| inspector_body(inspector, cx)))
                            .children(content),
                    ),
            )
            .into_any_element(),
        collection @ Some(_) => {
            main_with_inspector(toolbar, notices, collection, content, inspector, cx)
        }
        None if inspector.is_some() => {
            main_with_inspector(toolbar, notices, None, content, inspector, cx)
        }
        None => v_flex()
            .id("screen-page")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px_6()
            .pb_6()
            .when(staged, |page| page.pt_4())
            .gap_4()
            .children(toolbar)
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
                            .children(content),
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
    h_flex()
        .flex_none()
        .px_6()
        .pt_4()
        .pb_3()
        .gap_4()
        .items_center()
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
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
                .child(
                    h_flex()
                        .gap_2()
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
                .gap_2()
                .items_center()
                .children(header.actions),
        )
        .into_any_element()
}

fn stepper(stages: Stages, cx: &App) -> AnyElement {
    let t = &look(cx).tokens;
    let steps = stages
        .items
        .into_iter()
        .map(|item| step(item, stages.current, Rc::clone(&stages.on_pick), cx));
    h_flex()
        .id("stages")
        .flex_none()
        .px_6()
        .gap_1()
        .overflow_x_scroll()
        .border_b(t.border_width)
        .border_color(t.border)
        .children(steps)
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
    // readable width the row scrolls instead.
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

/// The main column, with the inspector on the right when there is one.
fn main_with_inspector(
    toolbar: Option<AnyElement>,
    notices: Vec<AnyElement>,
    collection: Option<Collection>,
    content: Vec<AnyElement>,
    inspector: Option<Inspector>,
    cx: &App,
) -> AnyElement {
    let t = &look(cx).tokens;
    let items = collection.map(|collection| collection_body(collection, cx));
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
                .children(toolbar)
                .children(notices)
                .children(items)
                .children(content),
        )
        .children(inspector.map(|inspector| {
            let footer = inspector.footer;
            v_flex()
                .w(px(INSPECTOR_WIDTH))
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
        }))
        .into_any_element()
}

fn inspector_body(inspector: Inspector, _cx: &App) -> AnyElement {
    v_flex()
        .gap_4()
        .children(inspector.title)
        .children(inspector.body)
        .children(inspector.footer)
        .into_any_element()
}

/// A grid or a feed, with its controls above.
fn collection_body(collection: Collection, cx: &App) -> AnyElement {
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
            .grid_cols(GRID_COLUMNS)
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
    v_flex()
        .id(collection.id)
        .gap_3()
        .children(controls)
        .child(items)
        .into_any_element()
}

/// A list of records in a column of its own, beside the open one.
fn list_column(collection: Collection, cx: &App) -> AnyElement {
    let t = &look(cx).tokens;
    let empty = collection.is_empty();
    v_flex()
        .w(px(LIST_WIDTH))
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
            v_flex()
                .id(collection.id)
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p_2()
                .gap_1()
                .when(empty, |list| {
                    list.child(div().p_1().children(collection.empty))
                })
                .children(collection.tiles.into_iter().map(|tile| row(tile, cx)))
                .children(collection.cards),
        )
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
