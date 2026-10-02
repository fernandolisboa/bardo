//! The editor screen (#19's approved design, `docs/design/editor.md`): top
//! bar, bin, preview and inspector, and the timeline below. It renders
//! `bardo_app::Editor` state only: the rough cut, the proxies' progress, the
//! playhead and the selection. Preview pictures come from `Editor::tick`,
//! called every frame while the preview plays; each becomes a `RenderImage`
//! and the previous one is dropped (ADR-0007).
//!
//! The editor keeps its own dark look whatever the rest of the app uses:
//! the design's tokens are below, and amber marks only the playhead, the
//! selection and the primary button.

mod timeline;

use std::sync::Arc;
use std::time::{Duration, Instant};

use bardo_app::bardo_domain::{FPS, Generation, VideoProjectId, timecode};
use bardo_app::{Bardo, ClipMedia, ClipProblem, ClipView, Editor, EditorView, PreviewAspect, Text};
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Entity, EventEmitter, FocusHandle, Hsla, ImageSource,
    KeyDownEvent, ObjectFit, RenderImage, SharedString, Task, Window, div, img, px, rgb,
};

use crate::shell::tr;

/// How often the screen checks the job queue for changes.
const POLL_EVERY: Duration = Duration::from_millis(100);

/// The design's tokens (`docs/design/editor.md`); the fills of tracks
/// that have no content yet come with their slices.
pub(crate) mod tokens {
    pub const APP: u32 = 0x0F1113;
    pub const PANEL: u32 = 0x16191C;
    pub const RAISED: u32 = 0x1E2226;
    pub const RAISED_HOVER: u32 = 0x262B30;
    pub const HAIRLINE: u32 = 0x2A2F35;
    pub const OUTLINE: u32 = 0x3A4048;
    pub const TEXT: u32 = 0xE7E9EC;
    pub const TEXT_2: u32 = 0xA3AAB3;
    pub const TEXT_3: u32 = 0x8A929C;
    pub const ACCENT: u32 = 0xF2A33A;
    pub const ACCENT_INK: u32 = 0x1A1206;
    pub const ERROR: u32 = 0xE5534B;
    pub const ERROR_FILL: u32 = 0x2A1416;
    pub const VIDEO_FILL: u32 = 0x2B3D54;
    pub const VIDEO_EDGE: u32 = 0x4F6E94;
    pub const NARRATION: u32 = 0x2FA295;
    pub const NARRATION_FILL: u32 = 0x14302D;
    pub const MUSIC: u32 = 0xBBAEF7;
    pub const SFX_EDGE: u32 = 0xC8664A;
    pub const CAPTIONS: u32 = 0xD2D6DC;
}

use tokens::*;

/// What the editor asks of the window around it.
pub enum EditorEvent {
    /// Back to the projects screen.
    Close,
    /// Show or hide the jobs panel.
    ToggleJobs,
}

pub struct EditorScreen {
    bardo: Entity<Bardo>,
    editor: Option<Editor>,
    error: Option<Text>,
    /// The picture shown in the preview.
    frame: Option<Arc<RenderImage>>,
    focus: FocusHandle,
    timeline: timeline::TimelineState,
    revision: u64,
    _poll: Task<()>,
}

impl EventEmitter<EditorEvent> for EditorScreen {}

impl EditorScreen {
    pub fn new(
        bardo: Entity<Bardo>,
        project: VideoProjectId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let opened = bardo.read(cx).open_editor(project);
        let (editor, error) = match opened {
            Ok(editor) => (Some(editor), None),
            Err(error) => (None, Some(error.message())),
        };
        let revision = bardo.read(cx).jobs_revision();
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL_EVERY).await;
                let alive = this.update(cx, |this, cx| this.poll(cx)).is_ok();
                if !alive {
                    break;
                }
            }
        });
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self {
            bardo,
            editor,
            error,
            frame: None,
            focus,
            timeline: timeline::TimelineState::default(),
            revision,
            _poll: poll,
        }
    }

    /// Reads the editor again when jobs moved (proxies, new scene media).
    fn poll(&mut self, cx: &mut Context<Self>) {
        let revision = self.bardo.read(cx).jobs_revision();
        if revision == self.revision {
            return;
        }
        self.revision = revision;
        let Self { bardo, editor, .. } = self;
        if let Some(editor) = editor.as_mut() {
            self.error = bardo
                .read(cx)
                .refresh_editor(editor)
                .err()
                .map(|error| error.message());
        }
        cx.notify();
    }

    fn with_editor(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut Editor)) {
        if let Some(editor) = self.editor.as_mut() {
            change(editor);
            cx.notify();
        }
    }

    fn toggle_play(&mut self, cx: &mut Context<Self>) {
        if let Some(editor) = self.editor.as_mut() {
            self.error = editor.toggle_play().err().map(|error| error.message());
            cx.notify();
        }
    }

    pub(crate) fn seek(&mut self, time: Duration, cx: &mut Context<Self>) {
        self.with_editor(cx, |editor| editor.seek(time));
    }

    pub(crate) fn select(&mut self, clip: Option<usize>, cx: &mut Context<Self>) {
        self.with_editor(cx, |editor| editor.select(clip));
    }

    fn retry_proxies(&mut self, cx: &mut Context<Self>) {
        let Self { bardo, editor, .. } = self;
        if let Some(editor) = editor.as_mut() {
            self.error = bardo
                .read(cx)
                .retry_proxies(editor)
                .err()
                .map(|error| error.message());
            cx.notify();
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "space" => self.toggle_play(cx),
            "left" => self.with_editor(cx, |editor| editor.step(-1)),
            "right" => self.with_editor(cx, |editor| editor.step(1)),
            "home" => self.seek(Duration::ZERO, cx),
            "end" => {
                let end = self
                    .editor
                    .as_ref()
                    .map_or(Duration::ZERO, |editor| editor.view().duration());
                self.seek(end, cx);
            }
            _ => {}
        }
    }

    /// Moves the preview on and swaps in a new picture when one is due.
    fn advance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.editor.as_mut() else {
            return;
        };
        if !editor.needs_ticks() {
            return;
        }
        if let Some(frame) = editor.tick(Instant::now()) {
            let size = frame.size;
            // RenderImage holds BGRA in an RGBA-typed buffer; no swizzle.
            if let Some(buffer) = image::RgbaImage::from_raw(size.width, size.height, frame.bgra) {
                let next = Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]));
                // Each frame is a new atlas entry; free the last one.
                if let Some(old) = self.frame.replace(next) {
                    cx.drop_image(old, Some(window));
                }
            }
        }
        window.request_animation_frame();
    }
}

fn color(hex: u32) -> Hsla {
    rgb(hex).into()
}

/// A text label in the design's small UI size.
pub(crate) fn label(text: impl Into<SharedString>, hex: u32) -> gpui_kit::Div {
    div()
        .text_size(px(12.))
        .text_color(color(hex))
        .whitespace_nowrap()
        .child(text.into())
}

/// A flat toolbar button in the editor's look: 32 px targets.
pub(crate) fn tool_button(
    id: impl Into<gpui_kit::ElementId>,
    enabled: bool,
    active: bool,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let base = div()
        .id(id)
        .h(px(28.))
        .min_w(px(28.))
        .px_2()
        .flex()
        .items_center()
        .justify_center()
        .gap_1()
        .rounded(px(4.))
        .text_size(px(12.));
    let base = if active {
        base.bg(color(RAISED_HOVER))
            .border_1()
            .border_color(color(OUTLINE))
            .text_color(color(TEXT))
    } else {
        base.text_color(color(if enabled { TEXT_2 } else { TEXT_3 }))
    };
    if enabled {
        base.cursor_pointer()
            .hover(|style| style.bg(color(RAISED_HOVER)))
    } else {
        base.opacity(0.5)
    }
}

fn icon(name: IconName, hex: u32) -> Icon {
    Icon::new(name).size_4().text_color(color(hex))
}

/// `m:ss` for scene and clip lengths.
fn short_duration(duration: Duration) -> String {
    let tenths = duration.as_millis() / 100;
    format!("{}:{:02}.{}", tenths / 600, tenths / 10 % 60, tenths % 10)
}

impl EditorScreen {
    fn render_top_bar(&self, view: Option<&EditorView>, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let mono = cx.theme().mono_font_family.clone();
        let back = tool_button("editor-back", true, false)
            .child(icon(IconName::ChevronLeft, TEXT_2))
            .child(tr(bardo, Text::EditorBack))
            .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(EditorEvent::Close)));
        let breadcrumb = view.map(|view| {
            h_flex()
                .gap_2()
                .min_w_0()
                .overflow_hidden()
                .child(label(view.channel_name.clone(), TEXT_2))
                .child(label("›", TEXT_3))
                .child(
                    label(view.project.title.clone(), TEXT)
                        .overflow_hidden()
                        .text_ellipsis(),
                )
        });
        let status = view.map(|view| {
            label(
                bardo.text_with(
                    Text::EditorDuration,
                    &[
                        ("duration", &short_duration(view.duration())),
                        ("fps", &FPS.to_string()),
                    ],
                ),
                TEXT_3,
            )
            .font_family(mono.clone())
        });
        let jobs_label = view.and_then(|view| {
            if view.is_building() {
                Some(bardo.text_with(
                    Text::EditorJobsProxies,
                    &[
                        ("ready", &view.proxies_ready.to_string()),
                        ("total", &view.proxies_total.to_string()),
                    ],
                ))
            } else if view
                .job
                .as_ref()
                .is_some_and(|job| job.state() == bardo_app::bardo_domain::JobState::Failed)
            {
                Some(bardo.text(Text::EditorJobFailed).into_owned())
            } else {
                None
            }
        });
        let failed = view.is_some_and(|view| !view.is_building()) && jobs_label.is_some();
        let jobs = tool_button("editor-jobs", true, false)
            .when(view.is_some_and(EditorView::is_building), |button| {
                button.child(div().size(px(6.)).rounded_full().bg(color(ACCENT)))
            })
            .when(failed, |button| {
                button.child(div().size(px(6.)).rounded_full().bg(color(ERROR)))
            })
            .child(
                jobs_label
                    .map(SharedString::from)
                    .unwrap_or_else(|| tr(bardo, Text::EditorJobsIdle)),
            )
            .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(EditorEvent::ToggleJobs)));
        // Undo, redo (#21) and render (#27) come with their slices.
        let undo = tool_button("editor-undo", false, false).child(icon(IconName::Undo, TEXT_3));
        let redo = tool_button("editor-redo", false, false).child(icon(IconName::Redo, TEXT_3));
        let render = div()
            .h(px(28.))
            .px_3()
            .flex()
            .items_center()
            .rounded(px(4.))
            .bg(color(ACCENT))
            .text_color(color(ACCENT_INK))
            .text_size(px(12.))
            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
            .opacity(0.5)
            .child(tr(bardo, Text::EditorReviewRender));
        h_flex()
            .h(px(44.))
            .flex_none()
            .px_3()
            .gap_3()
            .items_center()
            .bg(color(PANEL))
            .border_b_1()
            .border_color(color(HAIRLINE))
            .child(
                div()
                    .text_size(px(13.))
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .text_color(color(TEXT))
                    .child(tr(bardo, Text::AppName)),
            )
            .child(back)
            .children(breadcrumb)
            .child(div().flex_1())
            .children(status)
            .child(jobs)
            .child(undo)
            .child(redo)
            .child(render)
            .into_any_element()
    }

    fn render_bin(&self, view: Option<&EditorView>, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let mono = cx.theme().mono_font_family.clone();
        let selected_scene = self.editor.as_ref().and_then(|editor| {
            editor
                .selection()
                .map(|index| editor.view().clips[index].scene)
        });
        let tabs = h_flex()
            .gap_1()
            .px_2()
            .h(px(36.))
            .items_center()
            .border_b_1()
            .border_color(color(HAIRLINE))
            .child(tool_button("bin-scenes", true, true).child(tr(bardo, Text::EditorBinScenes)))
            .child(tool_button("bin-media", false, false).child(tr(bardo, Text::EditorBinMedia)))
            .child(
                tool_button("bin-styles", false, false)
                    .child(tr(bardo, Text::EditorBinCaptionStyles)),
            );
        let rows = view.map_or_else(Vec::new, |view| {
            view.scenes
                .iter()
                .map(|scene| {
                    let index = scene.index;
                    let clip = view.clips.iter().position(|clip| clip.scene == index);
                    let at = clip.map(|clip| view.clips[clip].at);
                    let selected = selected_scene == Some(index);
                    let thumbnail = match &scene.thumbnail {
                        Some(path) => img(path.clone())
                            .w(px(64.))
                            .h(px(36.))
                            .rounded(px(3.))
                            .object_fit(ObjectFit::Cover)
                            .into_any_element(),
                        None => div()
                            .w(px(64.))
                            .h(px(36.))
                            .rounded(px(3.))
                            .bg(color(RAISED))
                            .into_any_element(),
                    };
                    h_flex()
                        .id(("bin-scene", index))
                        .gap_2()
                        .p_2()
                        .rounded(px(4.))
                        .items_start()
                        .cursor_pointer()
                        .when(selected, |row| {
                            row.bg(color(RAISED)).border_1().border_color(color(ACCENT))
                        })
                        .hover(|style| style.bg(color(RAISED)))
                        .child(thumbnail)
                        .child(
                            v_flex()
                                .min_w_0()
                                .flex_1()
                                .gap_0p5()
                                .child(
                                    h_flex()
                                        .justify_between()
                                        .child(label(
                                            bardo.text_with(
                                                Text::EditorSceneLabel,
                                                &[("n", &(index + 1).to_string())],
                                            ),
                                            TEXT,
                                        ))
                                        .child(
                                            label(short_duration(scene.duration), TEXT_3)
                                                .font_family(mono.clone()),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(color(TEXT_3))
                                        .line_clamp(2)
                                        .child(SharedString::from(scene.prompt.clone())),
                                ),
                        )
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            if let Some(at) = at {
                                this.seek(at, cx);
                            }
                            this.select(clip, cx);
                        }))
                })
                .collect()
        });
        v_flex()
            .w(px(260.))
            .flex_none()
            .h_full()
            .bg(color(PANEL))
            .border_r_1()
            .border_color(color(HAIRLINE))
            .child(tabs)
            .child(
                v_flex()
                    .id("bin-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_2()
                    .gap_1()
                    .children(rows),
            )
            .into_any_element()
    }

    fn render_preview(&self, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let mono = cx.theme().mono_font_family.clone();
        let editor = self.editor.as_ref();
        let aspect = editor.map_or(PreviewAspect::Landscape, Editor::aspect);
        let aspects = h_flex()
            .gap_0p5()
            .p_0p5()
            .rounded(px(4.))
            .bg(color(APP))
            .children(PreviewAspect::ALL.map(|option| {
                let name = match option {
                    PreviewAspect::Landscape => bardo_app::bardo_domain::AspectRatio::Landscape,
                    PreviewAspect::Portrait => bardo_app::bardo_domain::AspectRatio::Vertical,
                };
                tool_button(
                    ("aspect", option as usize),
                    editor.is_some(),
                    option == aspect,
                )
                .h(px(24.))
                .child(tr(bardo, Text::AspectRatioName(name)))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.with_editor(cx, |editor| editor.set_aspect(option));
                }))
            }));
        let header = h_flex()
            .h(px(36.))
            .px_3()
            .gap_2()
            .items_center()
            .border_b_1()
            .border_color(color(HAIRLINE))
            .child(aspects)
            .child(div().flex_1())
            .child(
                div()
                    .px_1p5()
                    .py_0p5()
                    .rounded(px(3.))
                    .border_1()
                    .border_color(color(OUTLINE))
                    .child(label(
                        match editor.map(Editor::view) {
                            Some(view) if view.is_building() => format!(
                                "{} {}/{}",
                                bardo.text(Text::EditorProxyBadge),
                                view.proxies_ready,
                                view.proxies_total
                            ),
                            _ => bardo.text(Text::EditorProxyBadge).into_owned(),
                        },
                        TEXT_2,
                    )),
            );

        let ratio = match aspect {
            PreviewAspect::Landscape => 16. / 9.,
            PreviewAspect::Portrait => 9. / 16.,
        };
        let overlay = editor.and_then(|editor| self.preview_overlay(editor, cx));
        let picture = self.frame.clone().filter(|_| {
            editor.is_some_and(|editor| !editor.view().is_empty() && editor.can_play())
        });
        // The frame takes the stage's height at the preview's shape; on a
        // narrow stage it is clipped to the width and the picture letterboxed.
        let frame = div()
            .relative()
            .h_full()
            .max_w_full()
            .aspect_ratio(ratio)
            .overflow_hidden()
            .bg(color(0x000000))
            .border_1()
            .border_color(color(HAIRLINE))
            .children(picture.map(|image| {
                img(ImageSource::Render(image))
                    .absolute()
                    .inset_0()
                    .size_full()
                    .object_fit(ObjectFit::Contain)
            }))
            .children(overlay);
        let stage = div()
            .flex_1()
            .min_h_0()
            .p_4()
            .flex()
            .items_center()
            .justify_center()
            .bg(color(APP))
            .child(frame);

        let playing = editor.is_some_and(Editor::is_playing);
        let can_play = editor.is_some_and(Editor::can_play);
        let playhead = editor.map_or(Duration::ZERO, Editor::playhead);
        let duration = editor.map_or(Duration::ZERO, |editor| editor.view().duration());
        let transport = h_flex()
            .h(px(44.))
            .px_3()
            .gap_1()
            .items_center()
            .justify_center()
            .border_t_1()
            .border_color(color(HAIRLINE))
            .child(
                tool_button("previous-frame", can_play, false)
                    .child(icon(IconName::ChevronLeft, TEXT_2))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.with_editor(cx, |editor| editor.step(-1));
                    })),
            )
            .child(
                tool_button("play", can_play, false)
                    .w(px(36.))
                    .child(icon(
                        if playing {
                            IconName::Pause
                        } else {
                            IconName::Play
                        },
                        if can_play { TEXT } else { TEXT_3 },
                    ))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_play(cx))),
            )
            .child(
                tool_button("next-frame", can_play, false)
                    .child(icon(IconName::ChevronRight, TEXT_2))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.with_editor(cx, |editor| editor.step(1));
                    })),
            )
            .child(
                label(
                    format!("{} / {}", timecode(playhead), timecode(duration)),
                    TEXT_2,
                )
                .ml_3()
                .font_family(mono),
            );
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(color(PANEL))
            .child(header)
            .child(stage)
            .child(transport)
            .into_any_element()
    }

    /// What covers the preview: progress while proxies build, "Media
    /// offline" over a clip it cannot show, the empty state.
    fn preview_overlay(&self, editor: &Editor, cx: &App) -> Option<AnyElement> {
        let bardo = self.bardo.read(cx);
        let view = editor.view();
        let centered = || {
            v_flex()
                .absolute()
                .inset_0()
                .gap_2()
                .items_center()
                .justify_center()
                .bg(color(APP).opacity(0.85))
        };
        if view.is_empty() {
            return Some(
                centered()
                    .child(
                        label(timecode(Duration::ZERO), TEXT_3)
                            .font_family(cx.theme().mono_font_family.clone()),
                    )
                    .into_any_element(),
            );
        }
        if view.is_building() {
            let fraction = if view.proxies_total == 0 {
                0.0
            } else {
                view.proxies_ready as f32 / view.proxies_total as f32
            };
            return Some(
                centered()
                    .child(label(
                        bardo.text_with(
                            Text::EditorBuildingProxies,
                            &[
                                ("ready", &view.proxies_ready.to_string()),
                                ("total", &view.proxies_total.to_string()),
                            ],
                        ),
                        TEXT,
                    ))
                    .child(
                        div()
                            .w(px(220.))
                            .h(px(4.))
                            .rounded_full()
                            .bg(color(RAISED_HOVER))
                            .child(
                                div()
                                    .h_full()
                                    .rounded_full()
                                    .bg(color(ACCENT))
                                    .w(px(220. * fraction)),
                            ),
                    )
                    .child(label(tr(bardo, Text::EditorKeepEditing), TEXT_3))
                    .into_any_element(),
            );
        }
        let clip = view
            .clip_at(editor.playhead())
            .map(|index| &view.clips[index])?;
        if !matches!(clip.media, ClipMedia::Missing | ClipMedia::ProxyFailed(_)) {
            return None;
        }
        Some(
            centered()
                .child(icon(IconName::TriangleAlert, ERROR))
                .child(label(tr(bardo, Text::EditorMediaOffline), TEXT))
                .children(clip.file.clone().map(|file| label(file, TEXT_3)))
                .into_any_element(),
        )
    }

    fn render_inspector(&self, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let mono = cx.theme().mono_font_family.clone();
        let selected = self.editor.as_ref().and_then(|editor| {
            editor
                .selection()
                .map(|index| editor.view().clips[index].clone())
        });
        let body = match selected {
            None => div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .p_6()
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(color(TEXT_3))
                        .text_center()
                        .child(tr(bardo, Text::EditorInspectorEmpty)),
                )
                .into_any_element(),
            Some(clip) => {
                let field = |name: Text, value: String| {
                    h_flex()
                        .justify_between()
                        .gap_2()
                        .child(label(tr(bardo, name), TEXT_3))
                        .child(label(value, TEXT).font_family(mono.clone()))
                };
                let provenance = provenance_lines(bardo, &clip);
                v_flex()
                    .p_3()
                    .gap_3()
                    .when(clip.media == ClipMedia::Missing, |body| {
                        body.child(
                            h_flex()
                                .gap_2()
                                .p_2()
                                .rounded(px(4.))
                                .bg(color(ERROR_FILL))
                                .border_1()
                                .border_color(color(ERROR))
                                .child(icon(IconName::TriangleAlert, ERROR))
                                .child(label(tr(bardo, Text::EditorMediaMissing), TEXT)),
                        )
                    })
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                    .text_color(color(TEXT))
                                    .child(bardo.text_with(
                                        Text::EditorSceneLabel,
                                        &[("n", &(clip.scene + 1).to_string())],
                                    )),
                            )
                            .children(provenance.into_iter().map(|line| label(line, TEXT_2))),
                    )
                    .child(
                        v_flex()
                            .gap_1p5()
                            .child(field(Text::EditorIn, timecode(clip.at)))
                            .child(field(Text::EditorOut, timecode(clip.at + clip.duration)))
                            .child(field(Text::EditorLength, short_duration(clip.duration))),
                    )
                    .children(clip.file.clone().map(|file| {
                        v_flex()
                            .gap_1()
                            .child(label(tr(bardo, Text::EditorFile), TEXT_3))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(color(TEXT_2))
                                    .font_family(mono.clone())
                                    .child(SharedString::from(file)),
                            )
                    }))
                    .child(
                        v_flex()
                            .gap_1()
                            .child(label(tr(bardo, Text::EditorNarrationLabel), TEXT_3))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(color(TEXT_2))
                                    .child(SharedString::from(clip.text.clone())),
                            ),
                    )
                    .into_any_element()
            }
        };
        v_flex()
            .w(px(320.))
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
                    .child(label(tr(bardo, Text::EditorInspectorClip), TEXT_2)),
            )
            .child(body)
            .into_any_element()
    }

    /// The banner above the timeline: clips needing attention, and a plan
    /// older than the narration.
    fn render_banners(&self, view: &EditorView, cx: &mut Context<Self>) -> Option<AnyElement> {
        let bardo = self.bardo.read(cx);
        let problems = view.problems();
        // The screen's own error first, then the preview's (e.g. no sound).
        let error = self
            .error
            .or_else(|| self.editor.as_ref().and_then(Editor::error));
        if problems.is_empty() && !view.stale && error.is_none() {
            return None;
        }
        let can_retry = problems
            .iter()
            .any(|problem| !matches!(problem.media, ClipMedia::Missing));
        let problem_banner = (!problems.is_empty()).then(|| {
            let title = if problems.len() == 1 {
                bardo.text(Text::EditorProblemsOne).into_owned()
            } else {
                bardo.text_with(
                    Text::EditorProblemsMany,
                    &[("count", &problems.len().to_string())],
                )
            };
            h_flex()
                .gap_3()
                .px_3()
                .py_2()
                .items_start()
                .bg(color(ERROR_FILL))
                .border_b_1()
                .border_color(color(ERROR))
                .child(icon(IconName::TriangleAlert, ERROR))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_0p5()
                        .child(label(title, TEXT).font_weight(gpui_kit::FontWeight::SEMIBOLD))
                        .children(problems.iter().take(4).map(|problem| {
                            label(problem_line(bardo, problem), TEXT_2)
                                .overflow_hidden()
                                .text_ellipsis()
                        })),
                )
                .when(can_retry, |banner| {
                    banner.child(
                        tool_button("retry-proxies", true, true)
                            .child(tr(bardo, Text::EditorRetryProxies))
                            .on_click(
                                cx.listener(|this, _: &ClickEvent, _, cx| this.retry_proxies(cx)),
                            ),
                    )
                })
        });
        let notes = view
            .stale
            .then_some(Text::EditorStale)
            .into_iter()
            .chain(error)
            .map(|text| {
                h_flex()
                    .px_3()
                    .py_1p5()
                    .gap_2()
                    .bg(color(RAISED))
                    .border_b_1()
                    .border_color(color(HAIRLINE))
                    .child(icon(IconName::Info, TEXT_2))
                    .child(label(tr(bardo, text), TEXT_2))
            });
        Some(
            v_flex()
                .flex_none()
                .children(problem_banner)
                .children(notes)
                .into_any_element(),
        )
    }

    fn render_empty(&self, view: &EditorView, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        v_flex()
            .flex_1()
            .items_center()
            .justify_center()
            .gap_2()
            .bg(color(APP))
            .child(
                div()
                    .text_size(px(14.))
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .text_color(color(TEXT))
                    .child(tr(bardo, Text::EditorEmptyTitle)),
            )
            .child(label(
                tr(
                    bardo,
                    if view.has_narration {
                        Text::EditorEmptyNoScenes
                    } else {
                        Text::EditorEmptyNoNarration
                    },
                ),
                TEXT_2,
            ))
            .child(
                tool_button("empty-back", true, true)
                    .mt_2()
                    .child(tr(bardo, Text::EditorBackToProject))
                    .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(EditorEvent::Close))),
            )
            .into_any_element()
    }
}

/// Where the clip's picture comes from, for the inspector.
fn provenance_lines(bardo: &Bardo, clip: &ClipView) -> Vec<String> {
    let source = |text: Text, generation: &Generation| {
        bardo.text_with(
            text,
            &[
                (
                    "provider",
                    &bardo.text(Text::ProviderName(generation.provider)),
                ),
                ("model", &generation.model),
            ],
        )
    };
    let mut lines = Vec::new();
    match &clip.image {
        Some(image) => lines.push(source(Text::EditorSourceImage, image)),
        None => lines.push(bardo.text(Text::EditorSourceNone).into_owned()),
    }
    match &clip.clip {
        Some(generation) => lines.push(source(Text::EditorSourceClip, generation)),
        None if clip.image.is_some() => {
            lines.push(bardo.text(Text::EditorSourceStill).into_owned())
        }
        None => {}
    }
    lines
}

fn problem_line(bardo: &Bardo, problem: &ClipProblem) -> String {
    let reason = match (&problem.media, &problem.file) {
        (ClipMedia::ProxyFailed(detail), _) => detail.clone(),
        (ClipMedia::ProxyCancelled, _) => bardo.text(Text::EditorProblemCancelled).into_owned(),
        (_, Some(_)) => bardo.text(Text::EditorProblemFileGone).into_owned(),
        (_, None) => bardo.text(Text::EditorProblemNoImage).into_owned(),
    };
    bardo.text_with(
        Text::EditorProblemLine,
        &[
            ("n", &(problem.scene + 1).to_string()),
            ("file", problem.file.as_deref().unwrap_or("—")),
            ("reason", &reason),
        ],
    )
}

impl Render for EditorScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.advance(window, cx);
        let view = self.editor.as_ref().map(|editor| editor.view().clone());
        let top_bar = self.render_top_bar(view.as_ref(), cx);
        let root = v_flex()
            .id("editor")
            .track_focus(&self.focus)
            .size_full()
            .bg(color(APP))
            .text_color(color(TEXT))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| this.on_key(event, cx)))
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, _, window, cx| window.focus(&this.focus, cx)),
            )
            .child(top_bar);
        let Some(view) = view else {
            let bardo = self.bardo.read(cx);
            return root
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_center()
                        .children(self.error.map(|error| label(tr(bardo, error), ERROR))),
                )
                .into_any_element();
        };
        let middle = h_flex()
            .flex_1()
            .min_h_0()
            .child(self.render_bin(Some(&view), cx))
            .child(self.render_preview(cx))
            .child(self.render_inspector(cx));
        let banners = self.render_banners(&view, cx);
        let lower = if view.is_empty() {
            self.render_empty(&view, cx)
        } else {
            self.render_timeline(&view, window, cx)
        };
        root.child(middle)
            .children(banners)
            .child(
                div()
                    .h(gpui_kit::relative(0.4))
                    .flex_none()
                    .flex()
                    .flex_col()
                    .border_t_1()
                    .border_color(color(HAIRLINE))
                    .child(lower),
            )
            .into_any_element()
    }
}
