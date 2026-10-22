//! The published video's retention over the cut (#183,
//! `docs/guide/<language>/editor.md#retention`): the toolbar toggle, a row
//! under the ruler that draws the YouTube post's audience retention curve
//! at the cut's own times with its drops shaded and numbered, and the list
//! of drops that takes the inspector's place while it shows. The curve is
//! read when the editor opens (and again after a job, such as a metrics
//! sync) and lined up with the cut again whenever the cut changes
//! (`Bardo::retention_over`); a cut whose timing changed since the render
//! draws no curve, and the list says so. None of it is saved.

use std::time::Duration;

use bardo_app::bardo_domain::{Share, Timeline, time_at};
use bardo_app::{
    Bardo, Control, CutRetention, DropView, Editor, PostedRetention, Side, Text, TourAnchor,
};
use gpui_kit::component::{ActiveTheme as _, IconName, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, ClickEvent, MouseButton, PathBuilder, canvas, div, fill, point, px, size,
};

use super::tokens::*;
use super::{EditorScreen, color, icon, label, side_width, tool_button};
use crate::shell::tr;
use crate::tour::Anchored as _;

/// The row under the ruler's timecodes that draws the curve.
pub(super) const RETENTION_ROW: f32 = 40.;

/// The retention's view state: none of it is saved.
#[derive(Default)]
pub(super) struct RetentionState {
    /// Whether the row and the list show.
    pub(super) on: bool,
    /// The project's YouTube post and its curve, as last read.
    posted: Option<PostedRetention>,
    /// The curve over the cut, with the timeline it was lined up with.
    over: Option<(Option<Timeline>, CutRetention)>,
    /// The focused drop, by index.
    focused: Option<usize>,
}

impl RetentionState {
    fn over(&self) -> Option<&CutRetention> {
        self.over.as_ref().map(|(_, over)| over)
    }

    /// Whether the row shows under the ruler: on, with the curve lined up
    /// with the cut.
    pub(super) fn row(&self) -> bool {
        self.on
            && self
                .over()
                .is_some_and(|over| over.lined_up && over.length.is_some())
    }
}

/// A drop's span as `0:42–0:48`, or where it is in the video when its
/// length is not known.
pub(crate) fn drop_when(bardo: &Bardo, drop: &DropView) -> String {
    match drop.span {
        Some((start, end)) => format!(
            "{}–{}",
            bardo.clock(start.as_secs()),
            bardo.clock(end.as_secs().max(start.as_secs() + 1))
        ),
        None => bardo.text_with(
            Text::RetentionAtShare,
            &[("percent", &bardo.percent(drop.drop.from))],
        ),
    }
}

/// How many viewers leave in a drop: "6.5% of viewers leave (80% to 73.5%)".
pub(crate) fn drop_lost(bardo: &Bardo, drop: &DropView) -> String {
    bardo.text_with(
        Text::RetentionLost,
        &[
            ("lost", &bardo.percent(drop.drop.lost())),
            ("before", &bardo.percent(drop.drop.before)),
            ("after", &bardo.percent(drop.drop.after)),
        ],
    )
}

/// What plays where a drop is, when lined up: the scene (or footage) and
/// the sentence.
pub(crate) fn drop_place(bardo: &Bardo, drop: &DropView) -> (String, String) {
    let scene = match drop.scene {
        Some(scene) => bardo.text_with(Text::RetentionScene, &[("n", &(scene + 1).to_string())]),
        None => bardo.text(Text::RetentionFootage).into_owned(),
    };
    let sentence = match &drop.sentence {
        Some(sentence) => format!("“{}”", sentence.trim()),
        None => bardo.text(Text::RetentionNoSpeech).into_owned(),
    };
    (scene, sentence)
}

impl EditorScreen {
    /// Reads the project's YouTube post and its curve again: a metrics
    /// sync may have read a newer one.
    pub(super) fn load_retention(&mut self, cx: &mut Context<Self>) {
        let project = self.editor.as_ref().map(Editor::project);
        self.retention.posted = project
            .and_then(|project| self.bardo.read(cx).posted_retention(project).ok().flatten());
        self.retention.over = None;
    }

    /// Lines the curve up with the cut again once the cut changed.
    pub(super) fn refresh_retention(&mut self, cx: &mut Context<Self>) {
        let (Some(posted), Some(editor)) = (&self.retention.posted, &self.editor) else {
            self.retention.over = None;
            self.retention.focused = None;
            return;
        };
        let timeline = &editor.view().timeline;
        if self
            .retention
            .over
            .as_ref()
            .is_some_and(|(lined, _)| lined == timeline)
        {
            return;
        }
        let over = self.bardo.read(cx).retention_over(posted, editor.view());
        if self
            .retention
            .focused
            .is_some_and(|index| index >= over.drops.len() || !over.lined_up)
        {
            self.retention.focused = None;
        }
        self.retention.over = Some((timeline.clone(), over));
    }

    /// Shows or hides the curve and its list; the cut suggestions make way.
    pub(super) fn toggle_retention(&mut self, cx: &mut Context<Self>) {
        self.retention.on = !self.retention.on;
        self.retention.focused = None;
        if self.retention.on && self.cuts.on {
            self.toggle_cuts(cx);
        }
        cx.notify();
    }

    /// Shows the curve with the playhead on the drop that starts at `at`
    /// (Performance's "Show in editor").
    pub fn show_retention(&mut self, at: Option<Duration>, cx: &mut Context<Self>) {
        if !self.retention.on {
            self.toggle_retention(cx);
        }
        self.refresh_retention(cx);
        let index = at.and_then(|at| {
            self.retention
                .over()
                .filter(|over| over.lined_up)?
                .drops
                .iter()
                .position(|drop| drop.span.is_some_and(|(start, _)| start == at))
        });
        match index {
            Some(index) => self.focus_drop(index, cx),
            None => cx.notify(),
        }
    }

    /// Focuses a drop and puts the playhead at its start.
    fn focus_drop(&mut self, index: usize, cx: &mut Context<Self>) {
        let start = self
            .retention
            .over()
            .filter(|over| over.lined_up)
            .and_then(|over| over.drops.get(index))
            .and_then(|drop| drop.span)
            .map(|(start, _)| start);
        if let Some(start) = start {
            self.retention.focused = Some(index);
            self.seek(start, cx);
        }
    }

    /// The toolbar's toggle; on, it lights while a curve lines up.
    pub(super) fn render_retention_toggle(
        &self,
        editing: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let on = self.retention.on;
        let lined = self.retention.row();
        tool_button("toggle-retention", editing, on)
            .relative()
            .when(!on, |button| {
                button.border_1().border_color(color(HAIRLINE))
            })
            .child(
                div()
                    .size(px(6.))
                    .rounded_full()
                    .bg(color(if lined { ERROR } else { OUTLINE })),
            )
            .child(tr(bardo, Text::EditorRetention))
            .when(editing, |button| {
                button
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_retention(cx)))
            })
            .tour_anchor(
                TourAnchor::Control(Control::EditorRetention),
                Side::Above,
                None,
            )
            .into_any_element()
    }

    /// The curve's row under the ruler: the drops' bands, the curve, and a
    /// numbered chip on each drop, with the focused drop's edges marked
    /// down the tracks.
    pub(super) fn render_retention_row(
        &self,
        ruler: f32,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        if !self.retention.row() {
            return Vec::new();
        }
        let Some(over) = self.retention.over() else {
            return Vec::new();
        };
        let Some(length) = over.length else {
            return Vec::new();
        };
        let mono = cx.theme().mono_font_family.clone();
        let width = self.timeline.width();
        let top = ruler - RETENTION_ROW;
        let high = over
            .curve
            .points()
            .iter()
            .map(|point| point.watch)
            .max()
            .unwrap_or(Share::ONE)
            .max(Share::ONE)
            .ten_thousandths() as f32;
        let curve: Vec<(f32, f32)> = over
            .curve
            .points()
            .iter()
            .map(|point| {
                (
                    self.timeline.x(time_at(point.elapsed, length)),
                    point.watch.ten_thousandths() as f32 / high,
                )
            })
            .collect();
        let focused = self.retention.focused;
        let bands: Vec<(f32, f32, bool)> = over
            .drops
            .iter()
            .enumerate()
            .filter_map(|(index, drop)| {
                let (start, end) = drop.span?;
                Some((
                    self.timeline.x(start),
                    self.timeline.x(end),
                    focused == Some(index),
                ))
            })
            .collect();
        let (band, band_focused) = (color(ERROR_FILL), color(ACCENT).opacity(0.25));
        let (line, area) = (color(TEXT_2), color(TEXT_3).opacity(0.25));
        let drawing = canvas(
            |_, _, _| {},
            move |bounds, (), window, _| {
                let left = f32::from(bounds.origin.x);
                let top = f32::from(bounds.origin.y) + 4.;
                let height = f32::from(bounds.size.height) - 8.;
                for (from, to, focused) in &bands {
                    window.paint_quad(fill(
                        gpui_kit::Bounds::new(
                            point(px(left + from), bounds.origin.y),
                            size(px((to - from).max(2.)), bounds.size.height),
                        ),
                        if *focused { band_focused } else { band },
                    ));
                }
                let at =
                    |(x, share): (f32, f32)| point(px(left + x), px(top + height * (1. - share)));
                let (Some(first), Some(last)) = (curve.first(), curve.last()) else {
                    return;
                };
                let bottom = top + height;
                let mut shape = PathBuilder::fill();
                shape.move_to(point(px(left + first.0), px(bottom)));
                for point in &curve {
                    shape.line_to(at(*point));
                }
                shape.line_to(point(px(left + last.0), px(bottom)));
                shape.close();
                if let Ok(shape) = shape.build() {
                    window.paint_path(shape, area);
                }
                let mut stroke = PathBuilder::stroke(px(1.5));
                stroke.move_to(at(*first));
                for point in &curve[1..] {
                    stroke.line_to(at(*point));
                }
                if let Ok(stroke) = stroke.build() {
                    window.paint_path(stroke, line);
                }
            },
        )
        .absolute()
        .top(px(top))
        .left_0()
        .right_0()
        .h(px(RETENTION_ROW))
        .into_any_element();
        let mut marks = vec![drawing];
        // The focused drop's edges, down the tracks.
        if let Some((start, end)) = focused
            .and_then(|index| over.drops.get(index))
            .and_then(|drop| drop.span)
        {
            let edges = [self.timeline.x(start), self.timeline.x(end)];
            let ink = color(ACCENT);
            marks.push(
                canvas(
                    |_, _, _| {},
                    move |bounds, (), window, _| {
                        let left = f32::from(bounds.origin.x);
                        let top = f32::from(bounds.origin.y) + ruler;
                        let bottom = f32::from(bounds.origin.y + bounds.size.height);
                        for x in edges {
                            let mut y = top;
                            while y < bottom {
                                window.paint_quad(fill(
                                    gpui_kit::Bounds::new(
                                        point(px(left + x), px(y)),
                                        size(px(1.), px(4.0f32.min(bottom - y))),
                                    ),
                                    ink,
                                ));
                                y += 7.;
                            }
                        }
                    },
                )
                .absolute()
                .inset_0()
                .into_any_element(),
            );
        }
        for (index, drop) in over.drops.iter().enumerate() {
            let Some((start, _)) = drop.span else {
                continue;
            };
            let x = self.timeline.x(start);
            if x < -16. || x > width + 8. {
                continue;
            }
            let is_focused = focused == Some(index);
            marks.push(
                div()
                    .id(("retention-drop", index))
                    .absolute()
                    .top(px(top + 2.))
                    .left(px(x + 2.))
                    .h(px(14.))
                    .min_w(px(14.))
                    .px_0p5()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(3.))
                    .bg(color(if is_focused { ACCENT } else { RAISED }))
                    .border_1()
                    .border_color(color(if is_focused { ACCENT } else { ERROR }))
                    .text_size(px(10.))
                    .font_family(mono.clone())
                    .text_color(color(if is_focused { APP } else { ERROR }))
                    .cursor_pointer()
                    .child((index + 1).to_string())
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.focus_drop(index, cx);
                        }),
                    )
                    .into_any_element(),
            );
        }
        marks
    }

    /// The drops, in the inspector's place: where the curve comes from,
    /// whether it lines up with the cut, then a row per drop.
    pub(super) fn render_retention_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let mono = cx.theme().mono_font_family.clone();
        let small = |text: String, ink| div().text_size(px(12.)).text_color(color(ink)).child(text);
        let mut body: Vec<AnyElement> = Vec::new();
        match self.retention.over() {
            None => body.push(
                small(bardo.text(Text::RetentionNone).into_owned(), TEXT_2).into_any_element(),
            ),
            Some(over) => {
                body.push(
                    small(
                        bardo.text_with(
                            Text::RetentionRead,
                            &[("when", &bardo.time_ago(over.read_at))],
                        ),
                        TEXT_3,
                    )
                    .into_any_element(),
                );
                let warning = if over.length.is_none() {
                    Some(Text::RetentionNoLength)
                } else if !over.lined_up {
                    Some(Text::RetentionChanged)
                } else {
                    None
                };
                if let Some(warning) = warning {
                    body.push(
                        h_flex()
                            .gap_1p5()
                            .items_start()
                            .child(icon(IconName::TriangleAlert, ACCENT).size_3().mt_0p5())
                            .child(small(bardo.text(warning).into_owned(), TEXT_2).flex_1())
                            .into_any_element(),
                    );
                }
                body.push(
                    v_flex()
                        .gap_0p5()
                        .pt_2()
                        .border_t_1()
                        .border_color(color(HAIRLINE))
                        .child(
                            label(tr(bardo, Text::RetentionDrops), TEXT)
                                .font_weight(gpui_kit::FontWeight::SEMIBOLD),
                        )
                        .child(small(
                            bardo.text(Text::RetentionDropsHint).into_owned(),
                            TEXT_3,
                        ))
                        .into_any_element(),
                );
                if over.drops.is_empty() {
                    body.push(
                        small(bardo.text(Text::RetentionNoDrops).into_owned(), TEXT_3)
                            .into_any_element(),
                    );
                }
                for (index, drop) in over.drops.iter().enumerate() {
                    body.push(self.render_drop_row(bardo, over, index, drop, mono.clone(), cx));
                }
            }
        }
        v_flex()
            .w(px(side_width(cx)))
            .flex_none()
            .h_full()
            .bg(color(PANEL))
            .border_l_1()
            .border_color(color(HAIRLINE))
            .child(
                h_flex()
                    .h(px(36.))
                    .px_3()
                    .items_center()
                    .border_b_1()
                    .border_color(color(HAIRLINE))
                    .child(label(tr(bardo, Text::RetentionTitle), TEXT_2)),
            )
            .child(
                v_flex()
                    .id("retention-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_3()
                    .gap_2()
                    .children(body),
            )
            .into_any_element()
    }

    /// A drop in the list: its number and time, the viewers it loses, and
    /// what plays there while the curve lines up with the cut.
    fn render_drop_row(
        &self,
        bardo: &Bardo,
        over: &CutRetention,
        index: usize,
        drop: &DropView,
        mono: gpui_kit::SharedString,
        cx: &Context<Self>,
    ) -> AnyElement {
        let focused = self.retention.focused == Some(index);
        let clickable = over.lined_up && drop.span.is_some();
        let place = over.lined_up.then(|| drop_place(bardo, drop));
        h_flex()
            .id(("retention-row", index))
            .gap_2()
            .px_2()
            .py_1p5()
            .items_start()
            .rounded(px(4.))
            .when(focused, |row| {
                row.bg(color(RAISED)).border_1().border_color(color(ACCENT))
            })
            .when(!focused && clickable, |row| {
                row.hover(|style| style.bg(color(RAISED)))
            })
            .when(clickable, |row| {
                row.cursor_pointer().on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| this.focus_drop(index, cx)),
                )
            })
            .child(
                div()
                    .mt_0p5()
                    .h(px(16.))
                    .min_w(px(16.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(3.))
                    .border_1()
                    .border_color(color(ERROR))
                    .text_size(px(10.))
                    .font_family(mono.clone())
                    .text_color(color(ERROR))
                    .child((index + 1).to_string()),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(label(drop_when(bardo, drop), TEXT).font_family(mono))
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(color(TEXT_2))
                            .child(drop_lost(bardo, drop)),
                    )
                    .children(place.map(|(scene, sentence)| {
                        v_flex()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(color(TEXT_2))
                                    .child(scene),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(color(TEXT_3))
                                    .line_clamp(3)
                                    .child(sentence),
                            )
                    })),
            )
            .into_any_element()
    }
}
