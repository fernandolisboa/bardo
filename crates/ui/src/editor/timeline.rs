//! The editor's timeline (`docs/design/editor.md`, section 3): toolbar,
//! ruler, the five tracks with their 200 px headers, and the playhead.
//! It draws `EditorView` and the drag in progress; every edit goes through
//! `Bardo::edit` when the mouse is released.
//!
//! Dragging an item's edge trims it; dragging a clip drops it between two
//! others; dragging an audio item (narration, music, SFX) moves it. While
//! a drag runs, a ghost shows where the item will land (snapped, and
//! clamped as the domain will), and a line marks the word it snapped to.
//! Alt frees it from the words.
//!
//! Each audio lane's header carries its mute and solo buttons and its
//! level; clicking the header picks the lane, and the inspector shows its
//! mix. The music lane draws how the music ducks under the narration. The
//! music and SFX lanes show the imported audio placed there, with its
//! waveform.
//!
//! The caption lane shows each caption where the cut plays its words, as a
//! chip with its text; dragging a chip's edge retimes it.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use bardo_app::bardo_domain::{
    AudioItem, AudioLane, DUCK_RANGE, Decibels, DuckEnvelope, Ducking, Edge, ItemRef, LaneMix,
    Track as Lane, timecode,
};
use bardo_app::{ClipMedia, ClipView, EditAction, Editor, EditorView, Text};
use gpui_kit::component::{ActiveTheme as _, IconName, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Bounds, ClickEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ObjectFit, Pixels, ScrollWheelEvent, Window, canvas, div, fill, img, pattern_slash, point, px,
    size,
};

use super::tokens::*;
use super::{EditorScreen, clip_name, color, icon, label, tool_button};
use crate::appearance::EditorColor;
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
/// How close, on screen, a cut must come to a word to snap to it.
const SNAP_PX: f32 = 8.;
/// How far the mouse moves before a press becomes a drag.
const DRAG_PX: f32 = 3.;
/// The grab zone at each end of an item, for trimming.
const EDGE_PX: f32 = 6.;

/// What a drag holds of an item.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Grip {
    Edge(Edge),
    Body,
}

/// A drag on the timeline, from press to release.
#[derive(Clone, Copy)]
struct Drag {
    item: ItemRef,
    grip: Grip,
    /// Where the mouse was pressed, on screen and in time.
    from_x: f32,
    from: Duration,
    /// What the grip held when pressed: the edge's time, or the item's
    /// start.
    anchor: Duration,
    /// The time under the mouse now.
    time: Duration,
    /// Alt is held: no snapping.
    free: bool,
    moved: bool,
}

impl Drag {
    /// Where the grip is now: the anchor moved as far as the mouse.
    fn target(&self) -> Duration {
        if self.time >= self.from {
            self.anchor + (self.time - self.from)
        } else {
            self.anchor.saturating_sub(self.from - self.time)
        }
    }
}

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

    /// The audio lane it is, for the mix.
    fn lane(self) -> Option<AudioLane> {
        match self {
            Track::Narration => Some(AudioLane::Narration),
            Track::Music => Some(AudioLane::Music),
            Track::Sfx => Some(AudioLane::Sfx),
            Track::Captions | Track::Video => None,
        }
    }

    /// The track its items are edited on.
    fn item_track(self) -> Option<Lane> {
        match self {
            Track::Captions => Some(Lane::Captions),
            Track::Video => Some(Lane::Video),
            Track::Narration => Some(Lane::Narration),
            Track::Music => Some(Lane::Music),
            Track::Sfx => Some(Lane::Sfx),
        }
    }

    fn tint(self) -> EditorColor {
        match self {
            Track::Captions => CAPTIONS,
            Track::Video => VIDEO_EDGE,
            Track::Narration => NARRATION,
            Track::Music => MUSIC,
            Track::Sfx => SFX_EDGE,
        }
    }
}

/// How an audio lane draws its items.
#[derive(Clone, Copy)]
struct PieceLook {
    /// The element id's prefix.
    id: &'static str,
    fill: EditorColor,
    /// The waveform's and the outline's color.
    wave: EditorColor,
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
    drag: Option<Drag>,
}

impl Default for TimelineState {
    fn default() -> Self {
        Self {
            zoom: 40.,
            fit: true,
            scroll: 0.,
            lanes: Rc::default(),
            drag: None,
        }
    }
}

impl TimelineState {
    /// How far a cut may move to land on a word at this zoom.
    pub(crate) fn snap_reach(&self) -> Duration {
        Duration::from_secs_f32(SNAP_PX / self.zoom)
    }

    /// The reach for the drag in progress: none while Alt is held.
    fn drag_reach(&self, drag: &Drag) -> Duration {
        if drag.free {
            Duration::ZERO
        } else {
            self.snap_reach()
        }
    }

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

    /// Keeps a playing playhead in view: a page on when it runs off.
    fn follow(&mut self, playhead: Duration) {
        let x = self.x(playhead);
        let width = self.width();
        if width > 0. && (x < 0. || x > width - END_GAP) {
            self.scroll = (playhead.as_secs_f32() * self.zoom - width * 0.1).max(0.);
        }
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
        if let Some(editor) = self.editor.as_ref().filter(|editor| editor.is_playing()) {
            self.timeline.follow(editor.playhead());
            self.timeline.layout(view.duration());
        }
        let toolbar = self.render_timeline_toolbar(cx);
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
            .children(Track::ALL.map(|track| self.render_track_header(view, track, cx)));

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
                    self.render_clip(index, clip, selection == Some(ItemRef::video(index)), cx)
                })
                .collect::<Vec<_>>(),
        );
        let mut narration = Some(self.render_narration(view, selection, cx));
        let mut music = Some(self.render_music(view, selection, cx));
        let mut sfx = Some(self.render_sfx(view, selection, cx));
        let mut captions = Some(self.render_captions(view, selection, cx));
        let (mut ghost, snap_line) = self.render_drag(view);
        let lanes = Track::ALL.map(|track| {
            let lane = div()
                .relative()
                .h(px(track.height()))
                .overflow_hidden()
                .border_b_1()
                .border_color(color(HAIRLINE));
            let lane = match track {
                Track::Captions => lane.children(captions.take().into_iter().flatten()),
                Track::Video => lane.children(video.take().into_iter().flatten()),
                Track::Narration => lane.children(narration.take().into_iter().flatten()),
                Track::Music => lane.children(music.take().into_iter().flatten()),
                Track::Sfx => lane.children(sfx.take().into_iter().flatten()),
            };
            let ghost = ghost
                .take_if(|(on, _)| track.item_track() == Some(*on))
                .map(|(_, elements)| elements);
            lane.children(ghost.into_iter().flatten())
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
                    .children(snap_line)
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
                        if this.timeline.drag.is_some() {
                            this.drag_to(event, cx);
                        } else if event.pressed_button == Some(MouseButton::Left) {
                            let time = this.timeline.time_at(event.position.x);
                            this.seek(time, cx);
                        }
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseUpEvent, _, cx| this.drop_drag(cx)),
                    )
                    .on_mouse_up_out(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseUpEvent, _, cx| this.drop_drag(cx)),
                    )
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
        // The AI toggle arrives with #30.
        let toggle = |id: &'static str, text: Text| {
            tool_button(id, false, false)
                .border_1()
                .border_color(color(HAIRLINE))
                .child(tr(bardo, text))
        };
        let editing = self
            .editor
            .as_ref()
            .is_some_and(|e| e.view().timeline.is_some());
        let snapping = self.editor.as_ref().is_some_and(Editor::snapping);
        let snap = tool_button("toggle-snap", editing, snapping)
            .when(!snapping, |button| {
                button.border_1().border_color(color(HAIRLINE))
            })
            .child(div().size(px(6.)).rounded_full().bg(color(if snapping {
                TEXT
            } else {
                OUTLINE
            })))
            .child(tr(bardo, Text::EditorSnapWords))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.with_editor(cx, |editor| editor.set_snapping(!editor.snapping()));
            }));
        let ducking = self
            .editor
            .as_ref()
            .and_then(|editor| editor.view().timeline.as_ref())
            .map(|timeline| timeline.mix().ducking);
        let ducking_on = ducking.is_some_and(|ducking| ducking.on);
        let duck = tool_button("toggle-duck", editing, ducking_on)
            .when(!ducking_on, |button| {
                button.border_1().border_color(color(HAIRLINE))
            })
            .child(div().size(px(6.)).rounded_full().bg(color(if ducking_on {
                MUSIC
            } else {
                OUTLINE
            })))
            .child(tr(bardo, Text::EditorDuckMusic))
            .when_some(ducking, |button, ducking| {
                button.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.edit(
                        EditAction::SetDucking(Ducking {
                            on: !ducking.on,
                            ..ducking
                        }),
                        cx,
                    );
                }))
            });
        let shown = self
            .editor
            .as_ref()
            .and_then(|editor| editor.view().timeline.as_ref())
            .map(|timeline| timeline.captions().shown());
        let captions_on = shown == Some(true);
        let captions = tool_button("toggle-captions", editing, captions_on)
            .when(!captions_on, |button| {
                button.border_1().border_color(color(HAIRLINE))
            })
            .child(div().size(px(6.)).rounded_full().bg(color(if captions_on {
                CAPTIONS
            } else {
                OUTLINE
            })))
            .child(tr(bardo, Text::EditorShowCaptions))
            .when_some(shown, |button, shown| {
                button.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.edit(EditAction::ShowCaptions(!shown), cx);
                }))
            });
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
                        tool_button("tool-split", editing, false)
                            .child(tr(bardo, Text::EditorToolSplit))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                let reach = this.timeline.snap_reach();
                                this.edit(EditAction::SplitAtPlayhead { reach }, cx);
                            })),
                    )
                    .child(div().w(px(1.)).h(px(18.)).mx_1().bg(color(HAIRLINE)))
                    .child(snap)
                    .child(toggle("toggle-ai", Text::EditorAiCuts))
                    .child(duck)
                    .child(captions)
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

    /// A track's header: its chip and name, and for an audio lane its mute
    /// and solo buttons and its level (with the ducking on the music).
    /// Clicking an audio lane's header picks it for the inspector.
    fn render_track_header(
        &self,
        view: &EditorView,
        track: Track,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let mono = cx.theme().mono_font_family.clone();
        let chip = div()
            .px_1()
            .rounded(px(3.))
            .border_1()
            .border_color(color(track.tint()))
            .text_size(px(10.))
            .text_color(color(track.tint()))
            .child(track.chip());
        let header = h_flex()
            .h(px(track.height()))
            .px_2()
            .gap_2()
            .items_center()
            .border_b_1()
            .border_color(color(HAIRLINE));
        let (Some(lane), Some(mix)) = (track.lane(), view.timeline.as_ref().map(|t| *t.mix()))
        else {
            return header
                .child(chip)
                .child(label(tr(bardo, track.name()), TEXT_2))
                .into_any_element();
        };
        let lane_mix = mix.lane(lane);
        let picked = self
            .editor
            .as_ref()
            .is_some_and(|editor| editor.selected_lane() == Some(lane));
        let switch = |id: &'static str, short: Text, on: bool, set: LaneMix| {
            div()
                .id((id, lane as usize))
                .w(px(18.))
                .h(px(16.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(3.))
                .text_size(px(10.))
                .cursor_pointer()
                .border_1()
                .border_color(color(if on { TEXT_2 } else { OUTLINE }))
                .when(on, |button| button.bg(color(TEXT_2)).text_color(color(APP)))
                .when(!on, |button| {
                    button
                        .text_color(color(TEXT_3))
                        .hover(|style| style.bg(color(RAISED_HOVER)))
                })
                .child(tr(bardo, short))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.edit(EditAction::SetLane { lane, mix: set }, cx);
                }))
        };
        let mute = switch(
            "lane-mute",
            Text::EditorMuteShort,
            lane_mix.muted,
            LaneMix {
                muted: !lane_mix.muted,
                ..lane_mix
            },
        );
        let solo = switch(
            "lane-solo",
            Text::EditorSoloShort,
            lane_mix.solo,
            LaneMix {
                solo: !lane_mix.solo,
                ..lane_mix
            },
        );
        let readout = |text: String| {
            label(text, TEXT_3)
                .text_size(px(10.))
                .font_family(mono.clone())
                .overflow_hidden()
                .text_ellipsis()
        };
        let ducked = (lane == AudioLane::Music && mix.ducking.on).then(|| {
            let depth = bardo.decibels(Decibels::from_tenths(-mix.ducking.depth.tenths()));
            readout(bardo.text_with(Text::EditorDuckReadout, &[("depth", &depth)]))
        });
        let silent = !mix.is_audible(lane);
        header
            .id(("lane-header", lane as usize))
            .cursor_pointer()
            .relative()
            .when(picked, |header| {
                header.bg(color(RAISED)).child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .bottom_0()
                        .w(px(2.))
                        .bg(color(ACCENT)),
                )
            })
            .hover(|style| style.bg(color(RAISED)))
            .child(chip)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        label(
                            tr(bardo, track.name()),
                            if silent { TEXT_3 } else { TEXT_2 },
                        )
                        .overflow_hidden()
                        .text_ellipsis(),
                    )
                    .child(readout(bardo.decibels(lane_mix.gain)))
                    .children(ducked),
            )
            .child(mute)
            .child(solo)
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.with_editor(cx, |editor| editor.select_lane(Some(lane)));
            }))
            .into_any_element()
    }

    /// The music lane: the music placed there, or a note that there is
    /// none, with the ducking envelope drawn where it will lower it.
    fn render_music(
        &self,
        view: &EditorView,
        selection: Option<ItemRef>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut elements = self.render_audio_lane(view, AudioLane::Music, selection, cx);
        if elements.is_empty() {
            let bardo = self.bardo.read(cx);
            elements.push(
                div()
                    .absolute()
                    .left(px(8.))
                    .bottom(px(4.))
                    .child(label(tr(bardo, Text::EditorNoMusic), TEXT_3).text_size(px(11.)))
                    .into_any_element(),
            );
        }
        if let Some(envelope) = view.ducking.clone() {
            elements.push(self.envelope(envelope));
        }
        elements
    }

    /// The SFX lane: the sounds placed there.
    fn render_sfx(
        &self,
        view: &EditorView,
        selection: Option<ItemRef>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        self.render_audio_lane(view, AudioLane::Sfx, selection, cx)
    }

    /// The imported audio on the music or SFX lane, each item with its
    /// file's name and waveform.
    fn render_audio_lane(
        &self,
        view: &EditorView,
        lane: AudioLane,
        selection: Option<ItemRef>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(timeline) = &view.timeline else {
            return Vec::new();
        };
        let silent = !timeline.mix().is_audible(lane);
        let look = match lane {
            AudioLane::Music => PieceLook {
                id: "music",
                fill: MUSIC_FILL,
                wave: MUSIC,
            },
            AudioLane::Sfx | AudioLane::Narration => PieceLook {
                id: "sfx",
                fill: SFX_FILL,
                wave: SFX_EDGE,
            },
        };
        timeline
            .audio(lane)
            .iter()
            .enumerate()
            .filter_map(|(index, piece)| {
                let item = ItemRef::audio(lane, index);
                let media = view.media_file(&piece.file);
                let peaks = media.and_then(|media| media.peaks.clone());
                let name = media.map(|media| media.asset.name.clone());
                self.render_piece(
                    item,
                    piece,
                    peaks,
                    look,
                    name,
                    selection == Some(item),
                    silent,
                    cx,
                )
            })
            .collect()
    }

    /// The ducking envelope across the lane: a line at the top that dips as
    /// deep, in proportion, as the music will go.
    fn envelope(&self, envelope: DuckEnvelope) -> AnyElement {
        let timeline = &self.timeline;
        let (zoom, scroll, width) = (timeline.zoom, timeline.scroll, timeline.width());
        let depth = envelope.depth.db() / DUCK_RANGE.1.db();
        canvas(
            |_, _, _| {},
            move |bounds, (), window, _| {
                let top = f32::from(bounds.origin.y) + 6.;
                let room = f32::from(bounds.size.height) - 16.;
                let origin = f32::from(bounds.origin.x);
                let level = |x: f32| {
                    let time = Duration::from_secs_f32(((x + scroll) / zoom).max(0.));
                    top + envelope.amount_at(time) * depth * room
                };
                // Each step spans from the previous level so steep ramps stay joined.
                let (mut x, mut last) = (0., level(0.));
                while x < width {
                    let y = level(x);
                    let (high, low) = (y.min(last), y.max(last));
                    window.paint_quad(fill(
                        Bounds::new(
                            point(px(origin + x), px(high)),
                            size(px(2.), px(low - high + 1.5)),
                        ),
                        color(MUSIC),
                    ));
                    last = y;
                    x += 2.;
                }
            },
        )
        .absolute()
        .inset_0()
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
        let name = clip_name(bardo, clip);
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
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    let time = this.timeline.time_at(event.position.x);
                    this.seek(time, cx);
                    this.grab(ItemRef::video(index), Grip::Body, event, cx);
                }),
            )
            .children(self.edge_handles(ItemRef::video(index), width, cx));
        Some(element.into_any_element())
    }

    /// The grab zones at an item's two ends, for trimming.
    fn edge_handles(&self, item: ItemRef, width: f32, cx: &mut Context<Self>) -> Vec<AnyElement> {
        self.edge_handles_at(item, width, &[Edge::Start, Edge::End], cx)
    }

    /// The grab zones at some of an item's ends.
    fn edge_handles_at(
        &self,
        item: ItemRef,
        width: f32,
        edges: &[Edge],
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        if width < EDGE_PX * 3. {
            return Vec::new();
        }
        edges
            .iter()
            .copied()
            .map(|edge| {
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .w(px(EDGE_PX))
                    .when(edge == Edge::Start, |handle| handle.left_0())
                    .when(edge == Edge::End, |handle| handle.right_0())
                    .cursor_ew_resize()
                    .hover(|style| style.bg(color(TEXT).opacity(0.25)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            this.grab(item, Grip::Edge(edge), event, cx);
                        }),
                    )
                    .into_any_element()
            })
            .collect()
    }

    /// Starts a drag on `item` (selecting it); the lanes do not see the
    /// press.
    fn grab(&mut self, item: ItemRef, grip: Grip, event: &MouseDownEvent, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.select(Some(item), cx);
        let Some((at, duration)) = self.editor.as_ref().and_then(|e| e.view().span(item)) else {
            return;
        };
        let from = self.timeline.time_at(event.position.x);
        self.timeline.drag = Some(Drag {
            item,
            grip,
            from_x: event.position.x.into(),
            from,
            anchor: match grip {
                Grip::Edge(Edge::End) => at + duration,
                Grip::Edge(Edge::Start) | Grip::Body => at,
            },
            time: from,
            free: event.modifiers.alt,
            moved: false,
        });
    }

    fn drag_to(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let time = self.timeline.time_at(event.position.x);
        if let Some(drag) = self.timeline.drag.as_mut() {
            let x: f32 = event.position.x.into();
            drag.moved |= (x - drag.from_x).abs() > DRAG_PX;
            drag.time = time;
            drag.free = event.modifiers.alt;
            cx.notify();
        }
    }

    /// Ends the drag with the edit it shows.
    fn drop_drag(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.timeline.drag.take() else {
            return;
        };
        cx.notify();
        let Some(editor) = self.editor.as_ref().filter(|_| drag.moved) else {
            return;
        };
        let reach = self.timeline.drag_reach(&drag);
        let item = drag.item;
        let action = match (drag.grip, item.track) {
            (Grip::Edge(edge), _) => EditAction::Trim {
                item,
                edge,
                to: drag.target(),
                reach,
            },
            (Grip::Body, Lane::Video) => match editor.reorder_target(item.index, drag.time) {
                Some(to) if to != item.index => EditAction::Reorder {
                    from: item.index,
                    to,
                },
                _ => return,
            },
            (Grip::Body, Lane::Narration | Lane::Music | Lane::Sfx) => EditAction::Move {
                item,
                to: drag.target(),
            },
            // A caption keeps to its words; only its ends move.
            (Grip::Body, Lane::Captions) => return,
        };
        self.edit(action, cx);
    }

    /// The drag in progress: a ghost of the item where it will land, on
    /// the item's track, and the line of the word it snaps to.
    fn render_drag(
        &self,
        view: &EditorView,
    ) -> (Option<(Lane, Vec<AnyElement>)>, Option<AnyElement>) {
        let (Some(drag), Some(editor)) = (self.timeline.drag, self.editor.as_ref()) else {
            return (None, None);
        };
        if !drag.moved {
            return (None, None);
        }
        let timeline = &self.timeline;
        let ghost = |at: Duration, duration: Duration| {
            div()
                .absolute()
                .top(px(2.))
                .bottom(px(2.))
                .left(px(timeline.x(at)))
                .w(px((duration.as_secs_f32() * timeline.zoom).max(2.)))
                .rounded(px(3.))
                .border_2()
                .border_color(color(ACCENT))
                .bg(color(ACCENT).opacity(0.12))
                .into_any_element()
        };
        let reach = timeline.drag_reach(&drag);
        let mut snapped = None;
        let mut elements = Vec::new();
        match (drag.grip, drag.item.track) {
            (Grip::Edge(edge), _) => {
                if let Some((at, duration)) =
                    editor.trim_preview(drag.item, edge, drag.target(), reach)
                {
                    elements.push(ghost(at, duration));
                    let (cut, on_word) = editor.snapped(drag.target(), reach);
                    let edge_at = match edge {
                        Edge::Start => at,
                        Edge::End => at + duration,
                    };
                    snapped = (on_word && cut == edge_at).then_some(cut);
                }
            }
            (Grip::Body, Lane::Video) => {
                if let (Some((_, duration)), Some(to)) = (
                    view.span(drag.item),
                    editor.reorder_target(drag.item.index, drag.time),
                ) {
                    elements.push(ghost(drag.target(), duration));
                    // The slot it drops into.
                    let slot = if to > drag.item.index {
                        view.clips[to].at + view.clips[to].duration
                    } else {
                        view.clips[to].at
                    };
                    elements.push(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .left(px(timeline.x(slot) - 1.))
                            .w(px(3.))
                            .bg(color(ACCENT))
                            .into_any_element(),
                    );
                }
            }
            (Grip::Body, Lane::Narration | Lane::Music | Lane::Sfx) => {
                if let (Some((_, duration)), Some(at)) = (
                    view.span(drag.item),
                    editor.move_preview(drag.item, drag.target()),
                ) {
                    elements.push(ghost(at, duration));
                }
            }
            (Grip::Body, Lane::Captions) => {}
        }
        let line = snapped.map(|at| {
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(timeline.x(at)))
                .w(px(1.))
                .bg(color(TEXT))
                .into_any_element()
        });
        (Some((drag.item.track, elements)), line)
    }

    /// The caption lane: a chip with each caption's text wherever the cut
    /// plays its words, dimmed while the captions are off. Only a
    /// caption's first and last stretches carry its ends.
    fn render_captions(
        &self,
        view: &EditorView,
        selection: Option<ItemRef>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(timeline) = &view.timeline else {
            return Vec::new();
        };
        let captions = timeline.captions();
        let state = &self.timeline;
        // Each caption's first start and last end on the timeline.
        let mut hulls: HashMap<usize, (Duration, Duration)> = HashMap::new();
        for span in &view.captions {
            hulls
                .entry(span.index)
                .and_modify(|(start, end)| {
                    *start = (*start).min(span.at);
                    *end = (*end).max(span.end());
                })
                .or_insert((span.at, span.end()));
        }
        view.captions
            .iter()
            .enumerate()
            .filter_map(|(position, span)| {
                let left = state.x(span.at);
                let width = (span.duration.as_secs_f32() * state.zoom).max(2.);
                if left + width < 0. || left > state.width() {
                    return None;
                }
                let item = ItemRef::caption(span.index);
                let text = captions.lines().get(span.index)?.text.clone();
                let (start, end) = hulls[&span.index];
                let edges: Vec<Edge> = [
                    (span.at == start).then_some(Edge::Start),
                    (span.end() == end).then_some(Edge::End),
                ]
                .into_iter()
                .flatten()
                .collect();
                let selected = selection == Some(item);
                Some(
                    div()
                        .id(("caption", position))
                        .absolute()
                        .top(px(4.))
                        .bottom(px(4.))
                        .left(px(left))
                        .w(px(width))
                        .px_1()
                        .flex()
                        .items_center()
                        .overflow_hidden()
                        .rounded(px(3.))
                        .bg(color(CAPTIONS))
                        .when(selected, |chip| chip.border_2().border_color(color(ACCENT)))
                        .when(!captions.shown(), |chip| chip.opacity(0.4))
                        .child(
                            label(text, CAPTIONS_INK)
                                .text_size(px(11.))
                                .overflow_hidden()
                                .text_ellipsis(),
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                let time = this.timeline.time_at(event.position.x);
                                this.seek(time, cx);
                                this.grab(item, Grip::Body, event, cx);
                            }),
                        )
                        .children(self.edge_handles_at(item, width, &edges, cx))
                        .into_any_element(),
                )
            })
            .collect()
    }

    /// The narration lane: each piece of the narration where the cut
    /// plays it, with its stretch of the waveform, and the words on top.
    fn render_narration(
        &self,
        view: &EditorView,
        selection: Option<ItemRef>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let (Some(narration), Some(timeline)) = (&view.narration, &view.timeline) else {
            return Vec::new();
        };
        let silent = !timeline.mix().is_audible(AudioLane::Narration);
        let mut elements: Vec<AnyElement> = timeline
            .narration()
            .iter()
            .enumerate()
            .filter_map(|(index, piece)| {
                let item = ItemRef::narration(index);
                self.render_piece(
                    item,
                    piece,
                    narration.peaks.clone(),
                    PieceLook {
                        id: "narration",
                        fill: NARRATION_FILL,
                        wave: NARRATION,
                    },
                    None,
                    selection == Some(item),
                    silent,
                    cx,
                )
            })
            .collect();
        let state = &self.timeline;
        if state.zoom >= WORDS_FROM {
            elements.extend(
                narration
                    .words
                    .iter()
                    .enumerate()
                    .filter_map(|(index, word)| {
                        let x = state.x(word.start);
                        let end = state.x(word.end);
                        (end >= 0. && x <= state.width()).then(|| {
                            div()
                                .absolute()
                                .left(px(x))
                                .when(index % 2 == 0, |word| word.top(px(6.)))
                                .when(index % 2 == 1, |word| word.top(px(20.)))
                                .pl_0p5()
                                .border_l_1()
                                .border_color(color(NARRATION))
                                .child(label(word.text.clone(), TEXT_2).text_size(px(10.)))
                                .into_any_element()
                        })
                    }),
            );
        }
        elements
    }

    /// An audio item where the cut plays it: its stretch of the waveform,
    /// its fades and, when given, its file's name.
    #[allow(clippy::too_many_arguments)]
    fn render_piece(
        &self,
        item: ItemRef,
        piece: &AudioItem,
        peaks: Option<(u32, Vec<f32>)>,
        look: PieceLook,
        name: Option<String>,
        selected: bool,
        silent: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let timeline = &self.timeline;
        let left = timeline.x(piece.at);
        let width = (piece.duration.as_secs_f32() * timeline.zoom).max(2.);
        if left + width < 0. || left > timeline.width() {
            return None;
        }
        let zoom = timeline.zoom;
        let wave = look.wave;
        let source_start = piece.start.as_secs_f32();
        let (fade_in, fade_out) = piece.fades();
        let (fade_in, fade_out) = (fade_in.as_secs_f32() * zoom, fade_out.as_secs_f32() * zoom);
        // Drawn from the piece's own left edge, which may be off screen;
        // only the bars on screen are painted.
        let (from_x, to_x) = ((-left).max(0.), (timeline.width() - left).min(width));
        let waveform = canvas(
            |_, _, _| {},
            move |bounds, (), window, _| {
                let top: f32 = bounds.origin.y.into();
                let height: f32 = bounds.size.height.into();
                // On a whole pixel, so every piece's bars look alike.
                let origin = f32::from(bounds.origin.x).round();
                if let Some((per_second, peaks)) = peaks {
                    let middle = top + height / 2.;
                    let per_pixel = per_second as f32 / zoom;
                    let first = source_start * per_second as f32;
                    // One bar every 2 px of what is on screen.
                    let mut x = (from_x / 2.).floor() * 2.;
                    while x < to_x {
                        let from = (first + x * per_pixel) as usize;
                        let to = ((first + (x + 2.) * per_pixel) as usize).max(from + 1);
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
                            color(wave),
                        ));
                        x += 2.;
                    }
                }
                // The fades: a line rising from the bottom corner, and one
                // falling to the other.
                let bottom = top + height - 2.;
                let mut ramp = |from: f32, length: f32, rising: bool| {
                    let mut x = from.max(from_x);
                    while x < (from + length).min(to_x) {
                        let level = (x - from) / length;
                        let level = if rising { level } else { 1. - level };
                        window.paint_quad(fill(
                            Bounds::new(
                                point(px(origin + x), px(bottom - level * (height - 4.))),
                                size(px(2.), px(1.5)),
                            ),
                            color(TEXT),
                        ));
                        x += 2.;
                    }
                };
                if fade_in > 0. {
                    ramp(0., fade_in, true);
                }
                if fade_out > 0. {
                    ramp(width - fade_out, fade_out, false);
                }
            },
        )
        .absolute()
        .inset_0();
        Some(
            div()
                .id((look.id, item.index))
                .absolute()
                .top(px(4.))
                .bottom(px(4.))
                .left(px(left))
                .w(px(width))
                .overflow_hidden()
                .rounded(px(3.))
                .bg(color(look.fill))
                .border_1()
                .border_color(color(look.wave).opacity(0.6))
                .when(selected, |piece| {
                    piece.border_2().border_color(color(ACCENT))
                })
                .when(silent, |piece| piece.opacity(0.4))
                .child(waveform)
                .children(name.filter(|_| width > 48.).map(|name| {
                    div()
                        .absolute()
                        .top(px(2.))
                        .left(px(4.))
                        .right(px(4.))
                        .child(
                            label(name, TEXT)
                                .text_size(px(10.))
                                .overflow_hidden()
                                .text_ellipsis(),
                        )
                }))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        let time = this.timeline.time_at(event.position.x);
                        this.seek(time, cx);
                        this.grab(item, Grip::Body, event, cx);
                    }),
                )
                .children(self.edge_handles(item, width, cx))
                .into_any_element(),
        )
    }
}
