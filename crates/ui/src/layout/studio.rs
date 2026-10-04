//! The Studio layout (direction C of spike #52): denser than Workspace.
//! The places as tabs along the top, with jobs, the guide and settings on
//! the right and a status bar at the foot (jobs, the month's spend); each screen's
//! header in one line, the project stages as compact tabs with the stage's
//! actions beside them; scenes as table rows with the selected one in a
//! bottom panel of three columns; sections stacked rather than tabbed.

use std::rc::Rc;

use bardo_app::{Destination, Side, Stage, StageState, TourAnchor};
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Div, ElementId, Hsla, Stateful, div, px, relative};

use crate::appearance::look;
use crate::icons;
use crate::kit;
use crate::parts::{
    Collection, CollectionKind, Figure, Header, Inspector, NavItem, Navigation, OnPick,
    ScreenParts, Sections, StageItem, Stages, Tile,
};
use crate::tour::Anchored as _;

const TOP_BAR_HEIGHT: f32 = 44.;
const STATUS_BAR_HEIGHT: f32 = 28.;
/// A list of records beside the one open.
const LIST_WIDTH: f32 = 260.;
/// A form that drives a feed, beside it.
const FORM_WIDTH: f32 = 320.;
const ASIDE_WIDTH: f32 = 300.;
/// The longest a page's text runs.
const PAGE_WIDTH: f32 = 980.;
/// The bottom panel under a table.
const PANEL_HEIGHT: f32 = 256.;
/// Its last column: provenance.
const PANEL_SIDE_WIDTH: f32 = 240.;
/// Its middle column: the picture, or the current and new one.
const PANEL_MEDIA_WIDTH: f32 = 420.;
/// The widest the stage's actions get beside the stage tabs.
const TOOLBAR_WIDTH: f32 = 620.;
const ROW_HEIGHT: f32 = 48.;
const THUMB_WIDTH: f32 = 72.;
const THUMB_HEIGHT: f32 = 40.;
const NUMBER_WIDTH: f32 = 24.;
const TIME_WIDTH: f32 = 84.;
const FACT_WIDTH: f32 = 112.;

pub(super) fn shell(
    navigation: Navigation,
    screen: AnyElement,
    jobs: Option<AnyElement>,
    cx: &App,
) -> AnyElement {
    let t = &look(cx).tokens;
    let status = status_bar(&navigation, cx);
    v_flex()
        .size_full()
        .bg(t.app)
        .text_color(t.text)
        .child(top_bar(navigation, cx))
        .child(
            h_flex()
                .flex_1()
                .min_h_0()
                .w_full()
                .items_start()
                .child(div().flex_1().h_full().min_w_0().child(screen))
                .children(jobs),
        )
        .child(status)
        .into_any_element()
}

/// The places as tabs, pillar after pillar, then the pinned places that
/// are screens; jobs, the guide and settings as buttons on the right.
fn top_bar(nav: Navigation, cx: &App) -> AnyElement {
    let t = look(cx).tokens;
    let pick = nav.on_pick.clone();
    let tab = |item: &NavItem, selected: bool| {
        let place = item.place;
        let pick = Rc::clone(&pick);
        nav_tab(("nav", place as usize), selected, cx)
            .child(Icon::new(icons::destination(place)).size(px(14.)))
            .child(div().whitespace_nowrap().child(item.label.clone()))
            .on_click(move |_, window, cx| pick(place, window, cx))
    };
    let separator = || {
        div()
            .flex_none()
            .w(t.border_width.max(px(1.)))
            .h(px(16.))
            .mx_1()
            .bg(t.border)
    };

    let mut tabs: Vec<AnyElement> = Vec::new();
    for (ix, group) in nav.groups.iter().enumerate() {
        if ix > 0 {
            tabs.push(separator().into_any_element());
        }
        // A pillar's tabs together, so the tour can light them as one.
        tabs.push(
            h_flex()
                .flex_none()
                .gap_0p5()
                .items_center()
                .children(group.places.iter().map(|item| {
                    tab(item, item.place == nav.current).tour_anchor(
                        TourAnchor::NavPlace(item.place),
                        Side::Below,
                        Some(&nav.scroll),
                    )
                }))
                .tour_anchor(
                    TourAnchor::NavGroup(group.pillar),
                    Side::Below,
                    Some(&nav.scroll),
                )
                .into_any_element(),
        );
    }
    // Costs is a screen; jobs open a panel, the guide a menu, and settings
    // sit apart.
    let mut buttons: Vec<AnyElement> = Vec::new();
    for item in &nav.pinned {
        match item.place {
            Destination::Jobs => {
                let place = item.place;
                let pick = Rc::clone(&pick);
                buttons.push(
                    nav_tab(("nav", place as usize), nav.jobs_open, cx)
                        .child(Icon::new(icons::destination(place)).size(px(14.)))
                        .child(div().whitespace_nowrap().child(item.label.clone()))
                        .when(nav.jobs > 0, |button| {
                            button.child(
                                div()
                                    .px_1p5()
                                    .min_w(px(18.))
                                    .text_center()
                                    .rounded(t.radius)
                                    .bg(t.sunken)
                                    .border(t.border_width)
                                    .border_color(t.border)
                                    .text_xs()
                                    .child(nav.jobs.to_string()),
                            )
                        })
                        .on_click(move |_, window, cx| pick(place, window, cx))
                        .tour_anchor(TourAnchor::NavPlace(place), Side::Below, None)
                        .into_any_element(),
                );
            }
            // The Guide opens a menu over the screen, which stays; the
            // menu's "User guide" opens the Guide screen.
            Destination::Guide => buttons.push(
                tab(item, nav.guide_open || nav.current == Destination::Guide)
                    .tour_anchor(TourAnchor::NavPlace(item.place), Side::Below, None)
                    .into_any_element(),
            ),
            Destination::Settings => buttons.push(
                tab(item, nav.current == item.place)
                    .tour_anchor(TourAnchor::NavPlace(item.place), Side::Below, None)
                    .into_any_element(),
            ),
            _ => {
                tabs.push(separator().into_any_element());
                tabs.push(
                    tab(item, nav.current == item.place)
                        .tour_anchor(
                            TourAnchor::NavPlace(item.place),
                            Side::Below,
                            Some(&nav.scroll),
                        )
                        .into_any_element(),
                );
            }
        }
    }

    h_flex()
        .flex_none()
        .w_full()
        .h(px(TOP_BAR_HEIGHT))
        .px_3()
        .gap_1()
        .items_center()
        .bg(t.surface)
        .border_b(t.border_width)
        .border_color(t.border)
        .child(
            h_flex()
                .id("nav-tabs")
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .items_center()
                .overflow_x_scroll()
                .track_scroll(&nav.scroll)
                .children(tabs),
        )
        .child(
            h_flex()
                .flex_none()
                .gap_0p5()
                .items_center()
                .children(buttons),
        )
        .into_any_element()
}

/// A tab of the top bar; the current place is tinted and framed.
fn nav_tab(id: impl Into<ElementId>, selected: bool, cx: &App) -> Stateful<Div> {
    let t = look(cx).tokens;
    h_flex()
        .id(id)
        .flex_none()
        .h(px(28.))
        .px_2()
        .gap_1p5()
        .items_center()
        .rounded(t.radius)
        .border(t.border_width)
        .cursor_pointer()
        .text_sm()
        .font_weight(gpui_kit::FontWeight::MEDIUM)
        .map(|tab| {
            if selected {
                tab.bg(t.selected)
                    .border_color(t.accent_edge)
                    .text_color(t.accent_text)
            } else {
                tab.border_color(gpui_kit::transparent_black())
                    .text_color(t.text2)
                    .hover(|tab| tab.bg(t.hover).text_color(t.text))
            }
        })
}

/// Jobs on the left (a click opens their panel), the month's spend and
/// the budgets' use on the right.
fn status_bar(nav: &Navigation, cx: &App) -> AnyElement {
    let t = look(cx).tokens;
    let pick = Rc::clone(&nav.on_pick);
    let budgets = nav.budgets.as_ref().map(|budgets| {
        let ink = if budgets.tone == kit::Tone::Neutral {
            t.text2
        } else {
            budgets.tone.ink(cx)
        };
        h_flex()
            .gap_1p5()
            .items_center()
            .child(
                div()
                    .w(px(60.))
                    .h(px(4.))
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
            .child(budgets.line.clone())
    });
    h_flex()
        .flex_none()
        .w_full()
        .h(px(STATUS_BAR_HEIGHT))
        .px_3()
        .gap_4()
        .items_center()
        .bg(t.surface)
        .border_t(t.border_width)
        .border_color(t.border)
        .text_xs()
        .text_color(t.text2)
        .child(
            h_flex()
                .id("status-jobs")
                .gap_1p5()
                .items_center()
                .cursor_pointer()
                .hover(|item| item.text_color(t.text))
                .when(nav.jobs > 0, |item| item.text_color(t.text))
                .child(Icon::new(icons::destination(Destination::Jobs)).size(px(13.)))
                .child(nav.jobs_line.clone())
                .on_click(move |_, window, cx| pick(Destination::Jobs, window, cx)),
        )
        .child(div().flex_1())
        .children(budgets)
        .children(nav.spent_line.clone().map(|line| {
            div()
                .font_family(cx.theme().mono_font_family.clone())
                .text_color(t.text)
                .child(line)
        }))
        .into_any_element()
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
        summary,
        sections,
        scroll,
    } = parts;
    let t = &look(cx).tokens;

    // With stages, the toolbar sits beside them; otherwise on a row of
    // its own under the header.
    let (stage_row, toolbar) = match stages {
        Some(stages) => (Some(stage_tabs(stages, toolbar, cx)), None),
        None => (
            None,
            toolbar.map(|toolbar| kit::anchor(TourAnchor::Toolbar, toolbar).into_any_element()),
        ),
    };
    let mut content = content;
    if let Some(sections) = sections {
        content.extend(stacked(sections, cx));
    }

    let body = match collection {
        Some(collection) if collection.kind == CollectionKind::Grid => {
            table_with_panel(toolbar, notices, collection, content, inspector, cx)
        }
        Some(collection) if collection.kind == CollectionKind::List => h_flex()
            .flex_1()
            .min_h_0()
            .w_full()
            .items_start()
            .child(list_column(collection, cx))
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
                            .p_4()
                            .gap_3()
                            .children(toolbar)
                            .children(notices)
                            .children(inspector.map(|inspector| {
                                v_flex()
                                    .gap_3()
                                    .children(inspector.title)
                                    .children(inspector.body)
                                    .children(inspector.footer)
                                    .tour_anchor(TourAnchor::Inspector, Side::Below, None)
                            }))
                            .children(content),
                    ),
            )
            .when(!aside.is_empty(), |row| row.child(aside_panel(aside, cx)))
            .into_any_element(),
        collection @ Some(_) => {
            main_with_form(toolbar, notices, collection, content, inspector, cx)
        }
        None if inspector.is_some() => {
            main_with_form(toolbar, notices, None, content, inspector, cx)
        }
        None => h_flex()
            .flex_1()
            .min_h_0()
            .w_full()
            .items_start()
            .child(
                v_flex()
                    .id("screen-page")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .when_some(scroll, |page, scroll| page.track_scroll(&scroll))
                    .child(
                        v_flex()
                            .max_w(px(PAGE_WIDTH))
                            .p_4()
                            .gap_3()
                            .children(toolbar)
                            .children(notices)
                            .children(content),
                    )
                    .tour_anchor(TourAnchor::Content, Side::Right, None),
            )
            .when(!aside.is_empty(), |row| row.child(aside_panel(aside, cx)))
            .into_any_element(),
    };

    v_flex()
        .size_full()
        .bg(t.app)
        .text_color(t.text)
        .child(header_row(header, summary, cx))
        .children(stage_row)
        .child(body)
        .into_any_element()
}

/// Summaries beside the content, in a panel of their own.
fn aside_panel(aside: Vec<AnyElement>, cx: &App) -> AnyElement {
    let t = &look(cx).tokens;
    v_flex()
        .id("screen-aside")
        .w(px(ASIDE_WIDTH))
        .h_full()
        .flex_none()
        .overflow_y_scroll()
        .p_3()
        .gap_3()
        .bg(t.surface)
        .border_l(t.border_width)
        .border_color(t.border)
        .children(aside)
        .into_any_element()
}

/// The header in one line: the way here and the title, the meta, the
/// figures in brief, and the actions on the right. When the window is too
/// narrow for all of it, what does not fit goes to the next line (issue
/// #68): the figures never get narrower than their widest brief, so they
/// never run under the actions.
fn header_row(header: Header, summary: Vec<Figure>, cx: &App) -> AnyElement {
    let t = &look(cx).tokens;
    let mut trail = Vec::new();
    for crumb in header.trail {
        trail.push(crumb);
        trail.push(div().child("›").into_any_element());
    }
    let briefs = summary.into_iter().map(|figure| brief(figure, cx));
    h_flex()
        .flex_none()
        .w_full()
        .flex_wrap()
        .min_h(px(44.))
        .px_4()
        .py_1p5()
        .gap_x_3()
        .gap_y_1p5()
        .items_center()
        .border_b(t.border_width)
        .border_color(t.border)
        .child(
            h_flex()
                .flex_none()
                .gap_1()
                .items_center()
                .text_sm()
                .children((!trail.is_empty()).then(|| {
                    h_flex()
                        .gap_1()
                        .items_center()
                        .text_color(t.text2)
                        .children(trail)
                }))
                .child(
                    div()
                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                        .child(header.title),
                )
                .children(header.info),
        )
        .children(header.meta.map(|meta| {
            div()
                .min_w_0()
                .truncate()
                .text_xs()
                .text_color(t.text2)
                .child(meta)
        }))
        // No `min_w_0`: the automatic minimum keeps the widest brief whole.
        .child(
            h_flex()
                .flex_1()
                .gap_2()
                .items_center()
                .flex_wrap()
                .children(briefs),
        )
        .child(
            h_flex()
                .ml_auto()
                .flex_none()
                .gap_2()
                .items_center()
                .children(header.actions),
        )
        .tour_anchor(TourAnchor::Header, Side::Below, None)
        .into_any_element()
}

/// A figure in a line: its brief form, or its value and label.
fn brief(figure: Figure, cx: &App) -> AnyElement {
    if let Some(brief) = figure.brief {
        return brief;
    }
    let t = &look(cx).tokens;
    h_flex()
        .gap_1()
        .items_baseline()
        .text_sm()
        .child(
            div()
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .font_family(cx.theme().mono_font_family.clone())
                .child(figure.value),
        )
        .child(div().text_color(t.text2).child(figure.label))
        .into_any_element()
}

/// The stages as compact tabs with their notes, and the stage's actions
/// on the right.
fn stage_tabs(stages: Stages, toolbar: Option<AnyElement>, cx: &App) -> AnyElement {
    let t = &look(cx).tokens;
    let tabs = stages
        .items
        .into_iter()
        .map(|item| stage_tab(item, stages.current, Rc::clone(&stages.on_pick), cx));
    // The actions go under the tabs when both do not fit on one line, and
    // the tabs themselves wrap rather than run past the edge (issue #73).
    h_flex()
        .flex_none()
        .w_full()
        .flex_wrap()
        .px_3()
        .py_1p5()
        .gap_x_3()
        .gap_y_1p5()
        .items_center()
        .border_b(t.border_width)
        .border_color(t.border)
        .child(
            h_flex()
                .flex_wrap()
                .gap_0p5()
                .items_center()
                .children(tabs)
                .tour_anchor(TourAnchor::Stages, Side::Below, None),
        )
        .children(toolbar.map(|toolbar| {
            div()
                .ml_auto()
                .flex_none()
                .max_w(px(TOOLBAR_WIDTH))
                .child(toolbar)
                .tour_anchor(TourAnchor::Toolbar, Side::Below, None)
        }))
        .into_any_element()
}

fn stage_tab(item: StageItem, current: Stage, on_pick: OnPick<Stage>, cx: &App) -> AnyElement {
    let t = &look(cx).tokens;
    let stage = item.status.stage;
    let state = item.status.state;
    let on = stage == current;
    let locked = state == StageState::Locked;
    let (icon, ink): (Icon, Hsla) = match state {
        StageState::Done => (Icon::new(IconName::Check), t.success),
        StageState::Working => (Icon::new(IconName::LoaderCircle), t.info),
        StageState::Attention => (Icon::new(IconName::TriangleAlert), t.warning),
        StageState::Partial | StageState::Open => (Icon::new(icons::stage(stage)), t.text2),
        StageState::Locked => (Icon::new(gpui_kit::assets::IconName::Lock), t.text2),
    };
    h_flex()
        .id(("stage", stage as usize))
        .flex_none()
        .h(px(30.))
        .px_2()
        .gap_1p5()
        .items_center()
        .rounded(t.radius)
        .border(t.border_width)
        .text_sm()
        .map(|tab| {
            if on {
                tab.bg(t.selected)
                    .border_color(t.accent_edge)
                    .text_color(t.accent_text)
            } else {
                tab.border_color(gpui_kit::transparent_black())
                    .text_color(if locked { t.text2 } else { t.text })
            }
        })
        .when(!locked && !on, |tab| {
            tab.cursor_pointer().hover(|tab| tab.bg(t.hover))
        })
        .when(!locked, |tab| {
            tab.on_click(move |_, window, cx| on_pick(stage, window, cx))
        })
        .child(
            icon.size(px(13.))
                .text_color(if on { t.accent_text } else { ink }),
        )
        .child(
            div()
                .whitespace_nowrap()
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .child(item.name),
        )
        // A locked stage's lock says enough; its line waits in a tooltip.
        .map(|tab| {
            if locked {
                let note = item.note.clone();
                tab.tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(note.clone()).build(window, cx)
                })
            } else {
                tab.child(
                    div()
                        .whitespace_nowrap()
                        .px_1p5()
                        .rounded(t.radius)
                        .bg(t.sunken)
                        .text_xs()
                        .text_color(match state {
                            StageState::Attention => t.warning,
                            _ => t.text2,
                        })
                        .child(item.note),
                )
            }
        })
        .into_any_element()
}

/// The sections one under the other, each under its name.
fn stacked(sections: Sections, cx: &App) -> Vec<AnyElement> {
    let t = &look(cx).tokens;
    sections
        .items
        .into_iter()
        .map(|section| {
            v_flex()
                .gap_2()
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                        .text_color(t.text2)
                        .child(section.title.to_uppercase()),
                )
                .child(section.body)
                .into_any_element()
        })
        .collect()
}

/// A table of the items over the selected one's panel.
fn table_with_panel(
    toolbar: Option<AnyElement>,
    notices: Vec<AnyElement>,
    collection: Collection,
    content: Vec<AnyElement>,
    inspector: Option<Inspector>,
    cx: &App,
) -> AnyElement {
    let hint = collection.keys.as_ref().and_then(|keys| keys.hint.clone());
    let lead = toolbar.is_some() || !notices.is_empty();
    v_flex()
        .flex_1()
        .min_h_0()
        .w_full()
        .when(lead, |main| {
            main.child(
                v_flex()
                    .flex_none()
                    .px_4()
                    .py_2()
                    .gap_2()
                    .children(toolbar)
                    .children(notices),
            )
        })
        .child(table(collection, content, cx))
        .children(inspector.map(|inspector| panel(inspector, hint, cx)))
        .into_any_element()
}

/// The items as table rows under their column names, the rows scrolling
/// under the names, then `after` (provenance of the whole collection).
fn table(collection: Collection, after: Vec<AnyElement>, cx: &App) -> AnyElement {
    let t = look(cx).tokens;
    let Collection {
        id,
        controls,
        tiles,
        empty,
        headings,
        keys,
        ..
    } = collection;
    let after = (!after.is_empty()).then(|| v_flex().px_4().py_2().gap_2().children(after));
    let Some(first) = tiles.first() else {
        return v_flex()
            .id("table-empty")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px_4()
            .py_2()
            .gap_2()
            .when(!controls.is_empty(), |column| {
                column.child(h_flex().gap_2().items_center().children(controls))
            })
            .children(empty)
            .children(after)
            .tour_anchor(TourAnchor::Collection, Side::Below, None)
            .into_any_element();
    };
    let numbered = first.number.is_some();
    let pictured = first.picture.is_some() || tiles.iter().any(|tile| tile.picture.is_some());
    let timed = first.time.is_some();
    let detailed = tiles.iter().any(|tile| tile.detail.is_some());
    let facts: Vec<(gpui_kit::SharedString, bool)> = first
        .facts
        .iter()
        .map(|fact| (fact.label.clone(), fact.numeric))
        .collect();
    let stated = facts.is_empty() && tiles.iter().any(|tile| tile.status.is_some());

    let cell = |width: f32| div().w(px(width)).flex_none();
    let heading = |text: Option<gpui_kit::SharedString>| {
        text.map(|text| text.to_uppercase()).unwrap_or_default()
    };
    let head = h_flex()
        .px_4()
        .h(px(32.))
        .gap_3()
        .items_center()
        .text_xs()
        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
        .text_color(t.text2)
        .border_b(t.border_width)
        .border_color(t.border)
        .when(numbered, |row| row.child(cell(NUMBER_WIDTH).child("#")))
        .when(pictured, |row| {
            row.child(cell(THUMB_WIDTH).child(heading(headings.picture)))
        })
        .when(timed, |row| {
            row.child(cell(TIME_WIDTH).child(heading(headings.time)))
        })
        .child(div().flex_1().min_w_0().child(heading(headings.text)))
        .when(detailed, |row| {
            row.child(div().flex_1().min_w_0().child(heading(headings.detail)))
        })
        .children(facts.iter().map(|(label, numeric)| {
            cell(FACT_WIDTH)
                .flex()
                .when(*numeric, |cell| cell.justify_end())
                .child(label.to_uppercase())
        }))
        .when(stated, |row| row.child(cell(FACT_WIDTH)));

    let mono = cx.theme().mono_font_family.clone();
    let rows = tiles.into_iter().map(|tile| {
        let Tile {
            id,
            selected,
            picture,
            number,
            time,
            title,
            text,
            status,
            failed,
            attention,
            detail,
            facts,
            on_click,
            ..
        } = tile;
        let edge = if failed {
            t.danger
        } else if attention {
            t.accent_edge
        } else {
            t.border
        };
        h_flex()
            .id(id)
            .flex_none()
            .px_4()
            .h(px(ROW_HEIGHT))
            .gap_3()
            .items_center()
            .text_sm()
            .relative()
            .border_b(t.border_width)
            .border_color(t.border)
            .cursor_pointer()
            .map(|row| {
                if selected {
                    row.bg(t.selected).child(selection_bar(t.accent))
                } else {
                    row.hover(|row| row.bg(t.hover))
                }
            })
            .when(numbered, |row| {
                row.child(
                    cell(NUMBER_WIDTH)
                        .text_xs()
                        .text_color(t.text2)
                        .font_family(mono.clone())
                        .children(number),
                )
            })
            .when(pictured, |row| {
                row.child(
                    div()
                        .w(px(THUMB_WIDTH))
                        .h(px(THUMB_HEIGHT))
                        .flex_none()
                        .relative()
                        .overflow_hidden()
                        .rounded(t.radius)
                        .bg(t.sunken)
                        .border(t.border_width)
                        .border_color(edge)
                        .flex()
                        .items_center()
                        .justify_center()
                        .map(|thumb| match picture {
                            Some(picture) => thumb.child(div().absolute().inset_0().child(picture)),
                            None => thumb.child(
                                Icon::new(icons::Lucide::Image)
                                    .size(px(14.))
                                    .text_color(t.text2),
                            ),
                        }),
                )
            })
            .when(timed, |row| {
                row.child(
                    cell(TIME_WIDTH)
                        .text_xs()
                        .text_color(t.text2)
                        .font_family(mono.clone())
                        .children(time),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .children(title.map(|title| {
                        div()
                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                            .truncate()
                            .child(title)
                    }))
                    .children(text.map(|text| div().truncate().child(text))),
            )
            .when(detailed, |row| {
                row.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(t.text2)
                        .children(detail),
                )
            })
            .children(facts.into_iter().map(|fact| {
                cell(FACT_WIDTH)
                    .flex()
                    .min_w_0()
                    .overflow_hidden()
                    .when(fact.numeric, |cell| {
                        cell.justify_end().font_family(mono.clone())
                    })
                    .child(fact.value)
            }))
            .when(stated, |row| row.child(cell(FACT_WIDTH).children(status)))
            .on_click(move |event, window, cx| on_click(event, window, cx))
    });

    let scroll = keys.as_ref().map(|keys| keys.scroll.clone());
    super::take_keys(v_flex().id(id), keys)
        .flex_1()
        .min_h_0()
        .w_full()
        .when(!controls.is_empty(), |table| {
            table.child(
                h_flex()
                    .flex_none()
                    .px_4()
                    .py_2()
                    .gap_2()
                    .items_center()
                    .children(controls),
            )
        })
        .child(head.flex_none())
        .child(
            v_flex()
                .id("table-rows")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .when_some(scroll, |rows, scroll| rows.track_scroll(&scroll))
                .children(rows)
                .children(after),
        )
        .tour_anchor(TourAnchor::Collection, Side::Below, None)
        .into_any_element()
}

/// The selected item in three columns: its properties, its picture (or
/// what to review), and its provenance.
fn panel(inspector: Inspector, hint: Option<gpui_kit::SharedString>, cx: &App) -> AnyElement {
    let t = &look(cx).tokens;
    let Inspector {
        title,
        body,
        media,
        footer,
    } = inspector;
    let mut main = Vec::new();
    let mut picture = None;
    for (ix, part) in body.into_iter().enumerate() {
        if Some(ix) == media {
            picture = Some(part);
        } else {
            main.push(part);
        }
    }
    let column = |id: &'static str| {
        v_flex()
            .id(id)
            .h_full()
            .min_h_0()
            .overflow_y_scroll()
            .gap_3()
    };
    v_flex()
        .flex_none()
        .w_full()
        .h(px(PANEL_HEIGHT))
        .bg(t.surface)
        .border_t(t.border_width)
        .border_color(t.border_strong)
        .child(
            h_flex()
                .flex_none()
                .px_4()
                .pt_2()
                .pb_1()
                .gap_2()
                .items_center()
                .child(div().flex_1().min_w_0().children(title))
                .children(hint.map(|hint| kit::info("table-keys", None, hint))),
        )
        .child(
            h_flex()
                .flex_1()
                .min_h_0()
                .px_4()
                .pb_3()
                .gap_4()
                .items_start()
                .child(column("panel-main").flex_1().min_w_0().children(main))
                .children(picture.map(|picture| {
                    column("panel-media")
                        .flex_1()
                        .min_w_0()
                        .max_w(px(PANEL_MEDIA_WIDTH))
                        .child(picture)
                }))
                .children(footer.map(|footer| {
                    column("panel-side")
                        .w(px(PANEL_SIDE_WIDTH))
                        .flex_none()
                        .child(footer)
                })),
        )
        .tour_anchor(TourAnchor::Inspector, Side::Above, None)
        .into_any_element()
}

/// A feed (or a page) with the form that drives it on the right.
fn main_with_form(
    toolbar: Option<AnyElement>,
    notices: Vec<AnyElement>,
    collection: Option<Collection>,
    content: Vec<AnyElement>,
    inspector: Option<Inspector>,
    cx: &App,
) -> AnyElement {
    let t = &look(cx).tokens;
    let items = collection.map(|collection| feed(collection, cx));
    h_flex()
        .flex_1()
        .min_h_0()
        .w_full()
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
            v_flex()
                .id("inspector")
                .w(px(FORM_WIDTH))
                .h_full()
                .flex_none()
                .overflow_y_scroll()
                .p_3()
                .gap_3()
                .bg(t.surface)
                .border_l(t.border_width)
                .border_color(t.border)
                .children(inspector.title)
                .children(inspector.body)
                .children(inspector.footer)
                .tour_anchor(TourAnchor::Inspector, Side::Left, None)
        }))
        .into_any_element()
}

/// A feed's cards, with its controls above.
fn feed(collection: Collection, _cx: &App) -> AnyElement {
    let empty = collection.is_empty();
    super::take_keys(v_flex().id(collection.id), collection.keys)
        .gap_2()
        .when(!collection.controls.is_empty(), |feed| {
            feed.child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .items_center()
                    .children(collection.controls),
            )
        })
        .when(empty, |feed| feed.children(collection.empty))
        .children(collection.cards)
        .tour_anchor(TourAnchor::Collection, Side::Below, None)
        .into_any_element()
}

/// A list of records in a narrow column, a dense row each.
fn list_column(collection: Collection, cx: &App) -> AnyElement {
    let t = look(cx).tokens;
    let empty = collection.is_empty();
    let scroll = collection.keys.as_ref().map(|keys| keys.scroll.clone());
    let rows = collection.tiles.into_iter().flat_map(|mut tile| {
        let heading = tile.group.take().map(|group| {
            div()
                .px_3()
                .pt_2()
                .pb_1()
                .text_xs()
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .text_color(t.text2)
                .border_b(t.border_width)
                .border_color(t.border)
                .child(group.to_uppercase())
                .into_any_element()
        });
        heading
            .into_iter()
            .chain(std::iter::once(list_row(tile, cx)))
    });
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
                    .px_3()
                    .py_2()
                    .gap_2()
                    .flex_wrap()
                    .items_center()
                    .border_b(t.border_width)
                    .border_color(t.border)
                    .children(collection.controls),
            )
        })
        .child(
            super::take_keys(v_flex().id(collection.id), collection.keys)
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .when_some(scroll, |list, scroll| list.track_scroll(&scroll))
                .when(empty, |list| {
                    list.child(div().p_3().children(collection.empty))
                })
                .children(rows)
                .children(collection.cards),
        )
        .tour_anchor(TourAnchor::Collection, Side::Right, None)
        .into_any_element()
}

/// A record as one dense row: title and state, its line under it.
fn list_row(tile: Tile, cx: &App) -> AnyElement {
    let t = look(cx).tokens;
    let on_click = tile.on_click;
    v_flex()
        .id(tile.id)
        .px_3()
        .py_1p5()
        .gap_0p5()
        .relative()
        .border_b(t.border_width)
        .border_color(t.border)
        .cursor_pointer()
        .map(|row| {
            if tile.selected {
                row.bg(t.selected).child(selection_bar(t.accent))
            } else {
                row.hover(|row| row.bg(t.hover))
            }
        })
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .text_sm()
                .children(
                    tile.number
                        .map(|number| div().text_xs().text_color(t.text2).child(number)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .font_weight(gpui_kit::FontWeight::MEDIUM)
                        .children(tile.title),
                )
                .children(tile.marks)
                .children(tile.status),
        )
        .children(
            tile.text
                .map(|text| div().text_xs().text_color(t.text2).truncate().child(text)),
        )
        .children(
            tile.time
                .map(|time| div().text_xs().text_color(t.text2).child(time)),
        )
        .on_click(move |event, window, cx| on_click(event, window, cx))
        .into_any_element()
}

/// The edge that marks the selected row.
fn selection_bar(color: Hsla) -> Div {
    div()
        .absolute()
        .left_0()
        .top_0()
        .bottom_0()
        .w(px(3.))
        .bg(color)
}
