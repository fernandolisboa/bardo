//! The editor's timeline (`docs/design/editor.md`, section 3): toolbar,
//! ruler, the five tracks with their 200 px headers, and the playhead.
//! It draws `EditorView` only; editing tools come with #21 and later.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use bardo_app::bardo_domain::timecode;
use bardo_app::{ClipMedia, ClipView, EditorView, NarrationTrack, Text};
use gpui_kit::component::{ActiveTheme as _, IconName, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Bounds, ClickEvent, MouseButton, MouseDownEvent, MouseMoveEvent, ObjectFit, Pixels,
    ScrollWheelEvent, Window, canvas, div, fill, img, pattern_slash, point, px, size,
};

use super::tokens::*;
use super::{EditorScreen, color, icon, label, tool_button};
use crate::shell::tr;

/// The track header column, shared with the toolbar's timecode.
const HEADER: f32 = 200.;
const RULER: f32 = 24.;
/// Space kept after the last clip when the timeline fits the window.
const END_GAP: f32 = 24.;
/// Below this zoom the narration's words would overlap; they hide.
const WORDS_FROM: f32 = 60.;
const ZOOM_STEP: f32 = 1.5;
const MIN_ZOOM: f32 = 2.;
const MAX_ZOOM: f32 = 600.;

#[derive(Clone, Copy)]
enum Track {
    Captions,
    Video,
    Narration,
    Music,
    Sfx,
}

impl Track {
    const ALL: [Track; 5] = [
        Track::Captions,
        Track::Video,
        Track::Narration,
        Track::Music,
        Track::Sfx,
    ];

    fn height(self) -> f32 {
        match self {
            Track::Captions => 30.,
            Track::Video => 64.,
            Track::Narration => 82.,
            Track::Music => 54.,
            Track::Sfx => 34.,
        }
    }

    fn chip(self) -> &'static str {
        match self {
            Track::Captions => "CC",
            Track::Video => "V1",
            Track::Narration => "A1",
            Track::Music => "A2",
            Track::Sfx => "A3",
        }
    }

    fn name(self) -> Text {
        match self {
            Track::Captions => Text::EditorTrackCaptions,
            Track::Video => Text::EditorTrackVideo,
            Track::Narration => Text::EditorTrackNarration,
            Track::Music => Text::EditorTrackMusic,
            Track::Sfx => Text::EditorTrackSfx,
        }
    }

    fn tint(self) -> u32 {
        match self {
            Track::Captions => CAPTIONS,
            Track::Video => VIDEO_EDGE,
            Track::Narration => NARRATION,
            Track::Music => MUSIC,
            Track::Sfx => SFX_EDGE,
        }
    }
}

/// Zoom and scroll: view state the domain has no say in.
pub(crate) struct TimelineState {
    /// Pixels per second of timeline.
    zoom: f32,
    /// Whether the zoom follows the window so the whole cut shows.
    fit: bool,
    /// Pixels scrolled from the start.
    scroll: f32,
    /// Where the lanes were last drawn, for mouse positions and fitting.
    lanes: Rc<Cell<Bounds<Pixels>>>,
}

impl Default for TimelineState {
    fn default() -> Self {
        Self {
            zoom: 40.,
            fit: true,
            scroll: 0.,
            lanes: Rc::default(),
        }
    }
}

impl TimelineState {
    fn width(&self) -> f32 {
        self.lanes.get().size.width.into()
    }

    /// Settles zoom and scroll for a cut of `duration` in the current lanes.
    fn layout(&mut self, duration: Duration) {
        let width = self.width();
        let seconds = duration.as_secs_f32();
        if self.fit && width > END_GAP && seconds > 0. {
            self.zoom = ((width - END_GAP) / seconds).clamp(MIN_ZOOM, MAX_ZOOM);
        }
        let content = seconds * self.zoom + END_GAP;
        self.scroll = self.scroll.clamp(0., (content - width).max(0.));
    }

    fn x(&self, time: Duration) -> f32 {
        time.as_secs_f32() * self.zoom - self.scroll
    }

    /// The time under a window position.
    fn time_at(&self, position_x: Pixels) -> Duration {
        let left: f32 = self.lanes.get().origin.x.into();
        let x: f32 = position_x.into();
        Duration::from_secs_f32(((x - left + self.scroll) / self.zoom).max(0.))
    }

    fn zoom_by(&mut self, factor: f32) {
        self.fit = false;
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
    }

    /// Ruler step: the shortest that leaves room for a label.
    fn ruler_step(&self) -> u64 {
        [1, 2, 5, 10, 15, 30, 60, 120, 300, 600]
            .into_iter()
            .find(|step| *step as f32 * self.zoom >= 72.)
            .unwrap_or(1200)
    }
}

fn ruler_label(seconds: u64) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

impl EditorScreen {
    pub(super) fn render_timeline(
        &mut self,
        view: &EditorView,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.timeline.layout(view.duration());
        let toolbar = self.render_timeline_toolbar(cx);
        let bardo = self.bardo.read(cx);
        let headers = v_flex()
            .w(px(HEADER))
            .flex_none()
            .bg(color(PANEL))
            .border_r_1()
            .border_color(color(HAIRLINE))
            .child(
                div()
                    .h(px(RULER))
                    .border_b_1()
                    .border_color(color(HAIRLINE)),
            )
            .children(Track::ALL.map(|track| {
                h_flex()
                    .h(px(track.height()))
                    .px_2()
                    .gap_2()
                    .items_center()
                    .border_b_1()
                    .border_color(color(HAIRLINE))
                    .child(
                        div()
                            .px_1()
                            .rounded(px(3.))
                            .border_1()
                            .border_color(color(track.tint()))
                            .text_size(px(10.))
                            .text_color(color(track.tint()))
                            .child(track.chip()),
                    )
                    .child(label(tr(bardo, track.name()), TEXT_2))
            }));

        let lanes_bounds = self.timeline.lanes.clone();
        let measure = canvas(
            move |bounds, window, _| {
                if lanes_bounds.get() != bounds {
                    lanes_bounds.set(bounds);
                    // The zoom fits the lanes; draw again at their size.
                    window.refresh();
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0();

        let selection = self.editor.as_ref().and_then(|editor| editor.selection());
        let playhead = self
            .editor
            .as_ref()
            .map_or(Duration::ZERO, |editor| editor.playhead());
        let mut video = Some(
            view.clips
                .iter()
                .enumerate()
                .filter_map(|(index, clip)| {
                    self.render_clip(index, clip, selection == Some(index), cx)
                })
                .collect::<Vec<_>>(),
        );
        let mut narration = view
            .narration
            .as_ref()
            .and_then(|narration| self.render_narration(narration));
        let lanes = Track::ALL.map(|track| {
            let lane = div()
                .relative()
                .h(px(track.height()))
                .overflow_hidden()
                .border_b_1()
                .border_color(color(HAIRLINE));
            match track {
                Track::Video => lane.children(video.take().into_iter().flatten()),
                Track::Narration => lane.children(narration.take()),
                _ => lane,
            }
        });
        let playhead_x = self.timeline.x(playhead);
        let playhead_line = (playhead_x >= 0. && playhead_x <= self.timeline.width()).then(|| {
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(playhead_x))
                .w(px(1.))
                .bg(color(ACCENT))
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left(px(-5.))
                        .size(px(11.))
                        .rounded(px(2.))
                        .bg(color(ACCENT)),
                )
        });
        let body = h_flex()
            .flex_1()
            .min_h_0()
            .items_start()
            .child(headers)
            .child(
                v_flex()
                    .id("timeline-lanes")
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .cursor_pointer()
                    .child(measure)
                    .child(self.render_ruler(cx))
                    .children(lanes)
                    .children(playhead_line)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, _, cx| {
                            let time = this.timeline.time_at(event.position.x);
                            this.seek(time, cx);
                            this.select(None, cx);
                        }),
                    )
                    .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                        if event.pressed_button == Some(MouseButton::Left) {
                            let time = this.timeline.time_at(event.position.x);
                            this.seek(time, cx);
                        }
                    }))
                    .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                        let delta = event.delta.pixel_delta(px(16.));
                        let (dx, dy): (f32, f32) = (delta.x.into(), delta.y.into());
                        if event.modifiers.control {
                            this.timeline
                                .zoom_by(if dy > 0. { ZOOM_STEP } else { 1. / ZOOM_STEP });
                        } else {
                            this.timeline.scroll -= if dx.abs() > dy.abs() { dx } else { dy };
                        }
                        cx.notify();
                    })),
            );
        v_flex()
            .size_full()
            .bg(color(APP))
            .child(toolbar)
            .child(
                div()
                    .id("timeline-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(body),
            )
            .into_any_element()
    }

    fn render_timeline_toolbar(&self, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let mono = cx.theme().mono_font_family.clone();
        let playhead = self
            .editor
            .as_ref()
            .map_or(Duration::ZERO, |editor| editor.playhead());
        // Editing tools and toggles arrive with #21, #22, #25 and #30.
        let toggle = |id: &'static str, text: Text| {
            tool_button(id, false, false)
                .border_1()
                .border_color(color(HAIRLINE))
                .child(tr(bardo, text))
        };
        h_flex()
            .h(px(36.))
            .flex_none()
            .items_center()
            .bg(color(PANEL))
            .border_b_1()
            .border_color(color(HAIRLINE))
            .child(
                div()
                    .w(px(HEADER))
                    .flex_none()
                    .px_3()
                    .child(label(timecode(playhead), ACCENT).font_family(mono)),
            )
            .child(
                h_flex()
                    .flex_1()
                    .px_2()
                    .gap_1()
                    .items_center()
                    .child(
                        tool_button("tool-select", true, true)
                            .child(tr(bardo, Text::EditorToolSelect)),
                    )
                    .child(
                        tool_button("tool-split", false, false)
                            .child(tr(bardo, Text::EditorToolSplit)),
                    )
                    .child(div().w(px(1.)).h(px(18.)).mx_1().bg(color(HAIRLINE)))
                    .child(toggle("toggle-snap", Text::EditorSnapWords))
                    .child(toggle("toggle-ai", Text::EditorAiCuts))
                    .child(toggle("toggle-duck", Text::EditorDuckMusic))
                    .child(div().flex_1())
                    .child(
                        tool_button("zoom-out", true, false)
                            .child(icon(IconName::Minus, TEXT_2))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.timeline.zoom_by(1. / ZOOM_STEP);
                                cx.notify();
                            })),
                    )
                    .child(
                        tool_button("zoom-in", true, false)
                            .child(icon(IconName::Plus, TEXT_2))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.timeline.zoom_by(ZOOM_STEP);
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    fn render_ruler(&self, cx: &mut Context<Self>) -> AnyElement {
        let mono = cx.theme().mono_font_family.clone();
        let timeline = &self.timeline;
        let step = timeline.ruler_step();
        let first = (timeline.scroll / timeline.zoom) as u64 / step * step;
        let last = ((timeline.scroll + timeline.width()) / timeline.zoom) as u64 + step;
        let ticks = (first..=last).step_by(step as usize).map(|second| {
            let x = timeline.x(Duration::from_secs(second));
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(x))
                .pl_1()
                .border_l_1()
                .border_color(color(OUTLINE))
                .child(
                    label(ruler_label(second), TEXT_3)
                        .text_size(px(10.))
                        .font_family(mono.clone()),
                )
        });
        div()
            .relative()
            .h(px(RULER))
            .flex_none()
            .overflow_hidden()
            .bg(color(PANEL))
            .border_b_1()
            .border_color(color(HAIRLINE))
            .children(ticks)
            .into_any_element()
    }

    fn render_clip(
        &self,
        index: usize,
        clip: &ClipView,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let bardo = self.bardo.read(cx);
        let left = self.timeline.x(clip.at);
        let width = (clip.duration.as_secs_f32() * self.timeline.zoom).max(2.);
        if left + width < 0. || left > self.timeline.width() {
            return None;
        }
        let (fill_color, edge) = match clip.media {
            ClipMedia::Missing => (ERROR_FILL, ERROR),
            ClipMedia::ProxyFailed(_) | ClipMedia::ProxyCancelled => (ERROR_FILL, ERROR),
            _ => (VIDEO_FILL, VIDEO_EDGE),
        };
        let hatched = matches!(
            clip.media,
            ClipMedia::Building | ClipMedia::ProxyFailed(_) | ClipMedia::ProxyCancelled
        );
        let state = match &clip.media {
            ClipMedia::Ready => None,
            ClipMedia::Building => {
                Some((IconName::LoaderCircle, Text::EditorProxyBuilding, TEXT_2))
            }
            ClipMedia::Missing => Some((IconName::TriangleAlert, Text::EditorMediaMissing, ERROR)),
            ClipMedia::ProxyFailed(_) => {
                Some((IconName::TriangleAlert, Text::EditorProxyFailed, ERROR))
            }
            ClipMedia::ProxyCancelled => {
                Some((IconName::TriangleAlert, Text::EditorProxyCancelled, ERROR))
            }
        };
        let thumbnail = clip
            .thumbnail
            .clone()
            .filter(|_| width > 96. && clip.media != ClipMedia::Missing)
            .map(|path| {
                img(path)
                    .h_full()
                    .w(px(96.))
                    .flex_none()
                    .object_fit(ObjectFit::Cover)
                    .opacity(0.85)
            });
        let name = bardo.text_with(
            Text::EditorSceneLabel,
            &[("n", &(clip.scene + 1).to_string())],
        );
        let element = div()
            .id(("clip", index))
            .absolute()
            .top(px(4.))
            .bottom(px(4.))
            .left(px(left))
            .w(px(width))
            .flex()
            .overflow_hidden()
            .rounded(px(3.))
            .bg(color(fill_color))
            .when(hatched, |clip| {
                clip.bg(pattern_slash(color(edge).opacity(0.35), 2., 6.))
            })
            .border_1()
            .border_color(color(edge))
            .when(
                matches!(
                    clip.media,
                    ClipMedia::ProxyFailed(_) | ClipMedia::ProxyCancelled
                ),
                |clip| clip.border_dashed(),
            )
            .when(selected, |clip| clip.border_2().border_color(color(ACCENT)))
            .children(thumbnail)
            .child(
                v_flex()
                    .min_w_0()
                    .px_1p5()
                    .py_1()
                    .gap_0p5()
                    .child(label(name, TEXT).overflow_hidden().text_ellipsis())
                    .children(state.map(|(state_icon, text, tint)| {
                        h_flex()
                            .gap_1()
                            .child(icon(state_icon, tint).size_3())
                            .child(label(tr(bardo, text), tint).text_size(px(11.)))
                    })),
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.select(Some(index), cx)));
        Some(element.into_any_element())
    }

    fn render_narration(&self, narration: &NarrationTrack) -> Option<AnyElement> {
        let timeline = &self.timeline;
        let left = timeline.x(Duration::ZERO);
        let width = narration.duration.as_secs_f32() * timeline.zoom;
        if left + width < 0. {
            return None;
        }
        let zoom = timeline.zoom;
        let scroll = timeline.scroll;
        let peaks = narration.peaks.clone();
        let waveform = canvas(
            |_, _, _| {},
            move |bounds, (), window, _| {
                let Some((per_second, peaks)) = peaks else {
                    return;
                };
                let top: f32 = bounds.origin.y.into();
                let height: f32 = bounds.size.height.into();
                let origin: f32 = bounds.origin.x.into();
                let visible: f32 = bounds.size.width.into();
                let middle = top + height / 2.;
                let per_pixel = per_second as f32 / zoom;
                // One bar every 2 px of what is on screen.
                let mut x = 0.;
                while x < visible {
                    let from = ((x + scroll) * per_pixel) as usize;
                    let to = (((x + 2. + scroll) * per_pixel) as usize).max(from + 1);
                    let peak = peaks
                        .get(from..to.min(peaks.len()))
                        .unwrap_or_default()
                        .iter()
                        .fold(0f32, |max, peak| max.max(*peak));
                    let bar = (peak.clamp(0., 1.) * height * 0.9).max(1.);
                    window.paint_quad(fill(
                        Bounds::new(
                            point(px(origin + x), px(middle - bar / 2.)),
                            size(px(1.), px(bar)),
                        ),
                        color(NARRATION),
                    ));
                    x += 2.;
                }
            },
        )
        .absolute()
        .top_0()
        .bottom_0();
        let words = (zoom >= WORDS_FROM).then(|| {
            narration
                .words
                .iter()
                .enumerate()
                .filter_map(|(index, word)| {
                    let x = timeline.x(word.start);
                    let end = timeline.x(word.end);
                    (end >= 0. && x <= timeline.width()).then(|| {
                        div()
                            .absolute()
                            .left(px(x))
                            .when(index % 2 == 0, |word| word.top(px(2.)))
                            .when(index % 2 == 1, |word| word.top(px(16.)))
                            .pl_0p5()
                            .border_l_1()
                            .border_color(color(NARRATION))
                            .child(label(word.text.clone(), TEXT_2).text_size(px(10.)))
                    })
                })
                .collect::<Vec<_>>()
        });
        Some(
            div()
                .absolute()
                .top(px(4.))
                .bottom(px(4.))
                .left_0()
                .right_0()
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(px(left))
                        .w(px(width))
                        .rounded(px(3.))
                        .bg(color(NARRATION_FILL))
                        .border_1()
                        .border_color(color(NARRATION).opacity(0.6)),
                )
                .child(
                    waveform
                        .left(px(left.max(0.)))
                        .w(px((left + width).min(timeline.width()) - left.max(0.))),
                )
                .children(words.into_iter().flatten())
                .into_any_element(),
        )
    }
}
