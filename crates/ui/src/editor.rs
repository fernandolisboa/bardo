//! The editor screen (#19's approved design, `docs/design/editor.md`): top
//! bar, bin, preview and inspector, and the timeline below. It renders
//! `bardo_app::Editor` state only: the cut, the proxies' progress, the
//! playhead and the selection; edits go through `Bardo::edit`. Preview pictures come from `Editor::tick`,
//! called every frame while the preview plays; each becomes a `RenderImage`
//! and the previous one is dropped (ADR-0007).
//!
//! In a 9:16 cut, the selected clip's crop window shows over its whole
//! picture, dimmed around it, and dragging the window moves the crop; the
//! edit is made when the drag ends.
//!
//! A selected caption's text is edited in the inspector and applied on
//! Enter or when the field loses focus; the editor's shortcuts stay out of
//! the way while it has focus.
//!
//! The editor keeps its own dark look whatever the rest of the app uses:
//! the design's tokens are below, and amber marks only the playhead, the
//! selection and the primary button.

mod suggestions;
mod timeline;

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bardo_app::bardo_domain::{
    AspectRatio, AudioLane, CaptionStyle, CropPosition, DUCK_RANGE, Decibels, Ducking, Edge, FPS,
    Framing, GAIN_RANGE, Generation, ItemRef, LaneMix, MediaKind, PictureSize, Track,
    VideoProjectId, crop_window, frame_time, timecode,
};
use bardo_app::{
    Bardo, ClipMedia, ClipProblem, ClipShows, ClipView, Control, EditAction, Editor, EditorView,
    PREVIEW_LANDSCAPE, Side, Stage, Text, TourAnchor, caption_look,
};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Bounds, ClickEvent, Entity, EventEmitter, FocusHandle, Focusable as _, Hsla,
    ImageSource, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ObjectFit, PathPromptOptions, Pixels, RenderImage, SharedString, Subscription, Task, Window,
    canvas, div, img, px, relative, rgb,
};

use crate::appearance::{self, EditorColor};
use crate::guide;
use crate::icons::Lucide;
use crate::shell::tr;
use crate::tour::Anchored as _;

/// How often the screen checks the job queue for changes.
const POLL_EVERY: Duration = Duration::from_millis(100);
/// What one click of a level's − or + changes.
const GAIN_STEP: Decibels = Decibels::from_tenths(5);
const DUCK_STEP: Decibels = Decibels::from_tenths(10);
const FADE_STEP: Duration = Duration::from_millis(100);
/// What one click of the crop position's − or + moves: 5% of the room.
const CROP_STEP: i32 = 50;
/// A scene picture as the preview draws it (16:9), for placing a crop
/// window over it.
const SOURCE: PictureSize = PictureSize::new(PREVIEW_LANDSCAPE.0, PREVIEW_LANDSCAPE.1);

/// A crop window being dragged across its picture in the preview.
#[derive(Debug, Clone, Copy)]
struct CropDrag {
    /// The clip, by index on the video track.
    index: usize,
    from_x: f32,
    start: CropPosition,
    /// Where the window is now.
    position: CropPosition,
}

/// A clip's crop window as the preview shows it over its picture.
struct FramedWindow {
    index: usize,
    thumbnail: std::path::PathBuf,
    position: CropPosition,
    dragging: bool,
}

/// The design's palette (`docs/design/editor.md`) by role. The colors
/// follow the interface theme (`appearance::EditorTokens`): Graphite under
/// light and base dark themes, the theme's own under dark terminal and high
/// contrast themes.
pub(crate) mod tokens {
    pub use crate::appearance::EditorColor;
    pub const APP: EditorColor = EditorColor::App;
    pub const PANEL: EditorColor = EditorColor::Panel;
    pub const RAISED: EditorColor = EditorColor::Raised;
    pub const RAISED_HOVER: EditorColor = EditorColor::RaisedHover;
    pub const HAIRLINE: EditorColor = EditorColor::Hairline;
    pub const OUTLINE: EditorColor = EditorColor::Outline;
    pub const TEXT: EditorColor = EditorColor::Text;
    pub const TEXT_2: EditorColor = EditorColor::Text2;
    pub const TEXT_3: EditorColor = EditorColor::Text3;
    pub const ACCENT: EditorColor = EditorColor::Accent;
    pub const ERROR: EditorColor = EditorColor::Error;
    pub const ERROR_FILL: EditorColor = EditorColor::ErrorFill;
    pub const VIDEO_FILL: EditorColor = EditorColor::VideoFill;
    pub const VIDEO_EDGE: EditorColor = EditorColor::VideoEdge;
    pub const NARRATION: EditorColor = EditorColor::Narration;
    pub const NARRATION_FILL: EditorColor = EditorColor::NarrationFill;
    pub const MUSIC: EditorColor = EditorColor::Music;
    pub const MUSIC_FILL: EditorColor = EditorColor::MusicFill;
    pub const SFX_FILL: EditorColor = EditorColor::SfxFill;
    pub const SFX_EDGE: EditorColor = EditorColor::SfxEdge;
    pub const CAPTIONS: EditorColor = EditorColor::Captions;
    pub const CAPTIONS_INK: EditorColor = EditorColor::CaptionsInk;
}

use tokens::*;

/// The bin's tabs (caption styles live in the caption inspector).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BinTab {
    Scenes,
    Media,
}

/// The editor's key bindings.
pub fn init(cx: &mut App) {
    suggestions::init(cx);
}

/// What the editor asks of the window around it.
pub enum EditorEvent {
    /// Back to the projects screen.
    Close,
    /// Show or hide the jobs panel.
    ToggleJobs,
    /// Back to the projects screen, at the project's render review.
    Render,
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
    /// The selected caption's text, as the user types it.
    caption_input: Entity<InputState>,
    /// The caption the field holds, by index, and its text when loaded.
    caption_loaded: Option<(usize, String)>,
    crop_drag: Option<CropDrag>,
    /// Where the picture behind a crop window is drawn.
    framing_box: Rc<Cell<Bounds<Pixels>>>,
    bin_tab: BinTab,
    /// Files chosen for import and not done yet.
    importing: usize,
    /// The file dialog and the imports it started.
    import_task: Option<Task<()>>,
    /// Files the last import refused, by name, and why.
    import_errors: Vec<(String, Text)>,
    cuts: suggestions::CutsState,
    _poll: Task<()>,
    _subscriptions: Vec<Subscription>,
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
        let caption_input = cx.new(|cx| InputState::new(window, cx));
        let subscriptions = vec![cx.subscribe_in(
            &caption_input,
            window,
            |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } | InputEvent::Blur => {
                    this.apply_caption_text(window, cx)
                }
                _ => {}
            },
        )];
        Self {
            bardo,
            editor,
            error,
            frame: None,
            focus,
            timeline: timeline::TimelineState::default(),
            revision,
            caption_input,
            caption_loaded: None,
            crop_drag: None,
            framing_box: Rc::default(),
            bin_tab: BinTab::Scenes,
            importing: 0,
            import_task: None,
            import_errors: Vec::new(),
            cuts: suggestions::CutsState::default(),
            _poll: poll,
            _subscriptions: subscriptions,
        }
    }

    /// The selected caption: its index and text.
    fn selected_caption(&self) -> Option<(usize, String)> {
        let editor = self.editor.as_ref()?;
        let item = editor
            .selection()
            .filter(|item| item.track == Track::Captions)?;
        let caption = editor
            .view()
            .timeline
            .as_ref()?
            .captions()
            .lines()
            .get(item.index)?;
        Some((item.index, caption.text.clone()))
    }

    /// Saves what the caption field holds into the caption it was loaded
    /// from, if that caption is still there unchanged; a text the caption
    /// cannot take is put back as it was, and the banner says why.
    fn apply_caption_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((index, loaded)) = self.caption_loaded.clone() else {
            return;
        };
        let typed = self.caption_input.read(cx).value().to_string();
        if typed == loaded {
            return;
        }
        let Self { bardo, editor, .. } = self;
        let Some(editor) = editor.as_mut() else {
            return;
        };
        let current = editor
            .view()
            .timeline
            .as_ref()
            .and_then(|timeline| timeline.captions().lines().get(index))
            .map(|caption| caption.text.clone());
        if current.as_ref() != Some(&loaded) {
            return;
        }
        match bardo.read(cx).set_caption_text(editor, index, &typed) {
            Ok(()) => self.error = None,
            Err(error) => {
                self.error = Some(error.message());
                self.caption_input
                    .update(cx, |input, cx| input.set_value(loaded, window, cx));
            }
        }
        self.caption_loaded = None;
        cx.notify();
    }

    /// Loads the selected caption into the field when the selection or its
    /// text changes, after saving what was typed for the one before. What
    /// the user is typing stays while the field has focus.
    fn sync_caption_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.selected_caption();
        if selected == self.caption_loaded {
            return;
        }
        let same_caption = selected.as_ref().map(|(index, _)| index)
            == self.caption_loaded.as_ref().map(|(index, _)| index);
        let typing = self.caption_input.focus_handle(cx).is_focused(window);
        if same_caption && typing {
            return;
        }
        if !same_caption {
            self.apply_caption_text(window, cx);
            // Clicks on the timeline keep the focus where it was; once
            // another item is picked, the keys go back to the editor.
            if typing {
                window.focus(&self.focus, cx);
            }
        }
        let selected = self.selected_caption();
        let text = selected
            .as_ref()
            .map_or_else(String::new, |(_, text)| text.clone());
        self.caption_input
            .update(cx, |input, cx| input.set_value(text, window, cx));
        self.caption_loaded = selected;
    }

    /// Reads the editor again when jobs moved (proxies, new scene media).
    fn poll(&mut self, cx: &mut Context<Self>) {
        let revision = self.bardo.read(cx).jobs_revision();
        if revision == self.revision {
            return;
        }
        self.revision = revision;
        self.reload(cx);
    }

    /// Reads the editor again now, for media imported meanwhile.
    fn reload(&mut self, cx: &mut Context<Self>) {
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

    /// Asks for files and imports each one off the UI thread, showing it in
    /// the bin as soon as it is in.
    fn import_media(&mut self, cx: &mut Context<Self>) {
        let Some(project) = self.editor.as_ref().map(Editor::project) else {
            return;
        };
        let bardo = self.bardo.read(cx);
        let import = match bardo.media_import(project) {
            Ok(import) => import,
            Err(error) => {
                self.error = Some(error.message());
                cx.notify();
                return;
            }
        };
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some(tr(bardo, Text::EditorMediaImport)),
        });
        self.import_errors.clear();
        self.import_task = Some(cx.spawn(async move |this, cx| {
            let paths = match chosen.await {
                Ok(Ok(Some(paths))) => paths,
                Ok(Err(_)) => {
                    let _ = this.update(cx, |this, cx| {
                        this.error = Some(Text::FileDialogFailed);
                        this.import_task = None;
                        cx.notify();
                    });
                    return;
                }
                Ok(Ok(None)) | Err(_) => {
                    let _ = this.update(cx, |this, cx| {
                        this.import_task = None;
                        cx.notify();
                    });
                    return;
                }
            };
            let _ = this.update(cx, |this, cx| {
                this.importing = paths.len();
                cx.notify();
            });
            for path in paths {
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let import = import.clone();
                let result = cx
                    .background_executor()
                    .spawn(async move { import.run(&path) })
                    .await;
                let alive = this.update(cx, |this, cx| {
                    this.importing = this.importing.saturating_sub(1);
                    if let Err(error) = result {
                        this.import_errors.push((name, error.message()));
                    }
                    this.reload(cx);
                });
                if alive.is_err() {
                    return;
                }
            }
            let _ = this.update(cx, |this, cx| {
                this.import_task = None;
                cx.notify();
            });
        }));
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

    pub(crate) fn select(&mut self, item: Option<ItemRef>, cx: &mut Context<Self>) {
        self.with_editor(cx, |editor| editor.select(item));
    }

    /// Makes an edit and shows why when it is not made.
    pub(crate) fn edit(&mut self, action: EditAction, cx: &mut Context<Self>) {
        let Self { bardo, editor, .. } = self;
        if let Some(editor) = editor.as_mut() {
            self.error = bardo
                .read(cx)
                .edit(editor, action)
                .err()
                .map(|error| error.message());
            cx.notify();
        }
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

    fn on_key(&mut self, event: &KeyDownEvent, window: &Window, cx: &mut Context<Self>) {
        // Keys typed into the caption's text are the text's.
        if self.caption_input.focus_handle(cx).is_focused(window) {
            return;
        }
        let modifiers = event.keystroke.modifiers;
        let key = event.keystroke.key.as_str();
        if modifiers.control || modifiers.platform {
            match key {
                "z" if modifiers.shift => self.edit(EditAction::Redo, cx),
                "z" => self.edit(EditAction::Undo, cx),
                "y" => self.edit(EditAction::Redo, cx),
                _ => {}
            }
            return;
        }
        if modifiers.alt {
            match key {
                "left" => self.nudge_selection(-1, cx),
                "right" => self.nudge_selection(1, cx),
                _ => {}
            }
            return;
        }
        if self.cut_key(key, cx) {
            return;
        }
        let reach = self.timeline.snap_reach();
        match key {
            "space" => self.toggle_play(cx),
            "s" => self.edit(EditAction::SplitAtPlayhead { reach }, cx),
            "delete" | "backspace" => self.edit(EditAction::DeleteSelection, cx),
            "[" => self.trim_to_playhead(Edge::Start, cx),
            "]" => self.trim_to_playhead(Edge::End, cx),
            "escape" => self.select(None, cx),
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

    /// Moves the selected edge to the playhead (`[` and `]`).
    fn trim_to_playhead(&mut self, edge: Edge, cx: &mut Context<Self>) {
        let Some(editor) = self.editor.as_ref() else {
            return;
        };
        let Some(item) = editor.selection() else {
            self.error = Some(Text::EditorNothingToCut);
            cx.notify();
            return;
        };
        let to = editor.playhead();
        let reach = self.timeline.snap_reach();
        self.edit(
            EditAction::Trim {
                item,
                edge,
                to,
                reach,
            },
            cx,
        );
    }

    /// Alt+← and Alt+→: a clip one place earlier or later, an audio item
    /// one frame.
    fn nudge_selection(&mut self, by: i64, cx: &mut Context<Self>) {
        let Some(editor) = self.editor.as_ref() else {
            return;
        };
        let Some(item) = editor.selection() else {
            return;
        };
        let action = match item.track {
            Track::Video => {
                let Some(to) = item.index.checked_add_signed(by as isize) else {
                    return;
                };
                if to >= editor.view().clips.len() {
                    return;
                }
                EditAction::Reorder {
                    from: item.index,
                    to,
                }
            }
            Track::Narration | Track::Music | Track::Sfx => {
                let Some((at, _)) = editor.view().span(item) else {
                    return;
                };
                let to = if by < 0 {
                    at.saturating_sub(frame_time(1))
                } else {
                    at + frame_time(1)
                };
                EditAction::Move { item, to }
            }
            // A caption keeps to its words.
            Track::Captions => return,
        };
        self.edit(action, cx);
    }

    /// Frees the preview's picture from the window's atlas; the shell calls
    /// it before closing the editor.
    /// Whether there is a cut to show: its tour is offered only then.
    pub fn has_content(&self) -> bool {
        self.editor.as_ref().is_some_and(|editor| {
            editor
                .view()
                .timeline
                .as_ref()
                .is_some_and(|timeline| !timeline.is_empty())
        })
    }

    /// A tour step shows over the editor: playback pauses under the card,
    /// and nothing else changes.
    pub fn hold_for_tour(&mut self, cx: &mut Context<Self>) {
        self.with_editor(cx, Editor::hold_for_tour);
    }

    /// Takes the keyboard back, after the guide or a tour over the editor.
    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        window.focus(&self.focus, cx);
    }

    pub fn release(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(old) = self.frame.take() {
            cx.drop_image(old, Some(window));
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

fn color(ink: EditorColor) -> Hsla {
    appearance::editor_color(ink)
}

/// A text label in the design's small UI size.
pub(crate) fn label(text: impl Into<SharedString>, ink: EditorColor) -> gpui_kit::Div {
    div()
        .text_size(px(12.))
        .text_color(color(ink))
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

fn icon(name: IconName, ink: EditorColor) -> Icon {
    Icon::new(name).size_4().text_color(color(ink))
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
        let can_undo = self.editor.as_ref().is_some_and(Editor::can_undo);
        let can_redo = self.editor.as_ref().is_some_and(Editor::can_redo);
        let undo = tool_button("editor-undo", can_undo, false)
            .child(icon(IconName::Undo, if can_undo { TEXT_2 } else { TEXT_3 }))
            .when(can_undo, |button| {
                button.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.edit(EditAction::Undo, cx);
                }))
            });
        let redo = tool_button("editor-redo", can_redo, false)
            .child(icon(IconName::Redo, if can_redo { TEXT_2 } else { TEXT_3 }))
            .when(can_redo, |button| {
                button.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.edit(EditAction::Redo, cx);
                }))
            });
        // Hidden, not disabled, while there is nothing to render.
        let has_cut = view.is_some_and(|view| {
            view.timeline
                .as_ref()
                .is_some_and(|timeline| !timeline.is_empty())
        });
        let render = has_cut.then(|| {
            div()
                .id("editor-render")
                .relative()
                .h(px(28.))
                .px_3()
                .flex()
                .items_center()
                .rounded(px(4.))
                .text_size(px(12.))
                .bg(color(ACCENT))
                .text_color(color(APP))
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .cursor_pointer()
                .hover(|style| style.opacity(0.9))
                .child(tr(bardo, Text::EditorReviewRender))
                .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(EditorEvent::Render)))
                .tour_anchor(
                    TourAnchor::Control(Control::EditorRender),
                    Side::Below,
                    None,
                )
        });
        // The editor's tour, part one or two, once there is a cut to show.
        let tour = bardo.stage_tour(Stage::Edit, has_cut).map(|tour| {
            let started = tour.tour;
            tool_button("editor-tour", true, false)
                .child(
                    Icon::new(Lucide::BookOpen)
                        .size_4()
                        .text_color(color(TEXT_2)),
                )
                .child(tr(bardo, Text::TourName(tour.tour)))
                .when(tour.new, |button| {
                    button.child(
                        div()
                            .ml_1()
                            .px_1()
                            .rounded(px(3.))
                            .border_1()
                            .border_color(color(ACCENT))
                            .text_size(px(10.))
                            .text_color(color(ACCENT))
                            .child(tr(bardo, Text::TourNew)),
                    )
                })
                .tooltip(|window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new("Shift+F1").build(window, cx)
                })
                .on_click(move |_, window, cx| guide::start_tour(started, window, cx))
        });
        h_flex()
            .h(px(44.))
            .flex_none()
            .px_3()
            .gap_3()
            .items_center()
            .bg(color(PANEL))
            .border_b_1()
            .border_color(color(HAIRLINE))
            .child(back)
            .children(breadcrumb)
            .child(div().flex_1())
            .children(status)
            .children(tour)
            .child(jobs)
            .child(
                h_flex()
                    .relative()
                    .gap_3()
                    .child(undo)
                    .child(redo)
                    .tour_anchor(TourAnchor::Control(Control::EditorUndo), Side::Below, None),
            )
            .children(render)
            .into_any_element()
    }

    fn render_bin(&self, view: Option<&EditorView>, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let mono = cx.theme().mono_font_family.clone();
        let selected_scene = self.editor.as_ref().and_then(|editor| {
            editor
                .selected_clip()
                .and_then(|index| editor.view().clips[index].shows.scene())
        });
        let tab = |id: &'static str, text: Text, which: BinTab| {
            tool_button(id, true, self.bin_tab == which)
                .child(tr(bardo, text))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.bin_tab = which;
                    cx.notify();
                }))
        };
        let tabs = h_flex()
            .gap_1()
            .px_2()
            .h(px(36.))
            .items_center()
            .border_b_1()
            .border_color(color(HAIRLINE))
            .child(tab("bin-scenes", Text::EditorBinScenes, BinTab::Scenes))
            .child(tab("bin-media", Text::EditorBinMedia, BinTab::Media))
            .child(
                tool_button("bin-styles", false, false)
                    .child(tr(bardo, Text::EditorBinCaptionStyles)),
            );
        if self.bin_tab == BinTab::Media {
            return self.render_media_bin(tabs, view, cx);
        }
        let rows = view.map_or_else(Vec::new, |view| {
            view.scenes
                .iter()
                .map(|scene| {
                    let index = scene.index;
                    let clip = view
                        .clips
                        .iter()
                        .position(|clip| clip.shows == ClipShows::Scene(index));
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
                            this.select(clip.map(ItemRef::video), cx);
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

    /// The Media tab: import files, see each one with its length and state,
    /// and put it on a track at the playhead.
    fn render_media_bin(
        &self,
        tabs: gpui_kit::Div,
        view: Option<&EditorView>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let mono = cx.theme().mono_font_family.clone();
        let editing = view.is_some_and(|view| view.timeline.is_some());
        let busy = self.import_task.is_some();
        let import = tool_button("media-import", !busy, false)
            .border_1()
            .border_color(color(OUTLINE))
            .child(icon(IconName::Plus, TEXT_2))
            .child(tr(bardo, Text::EditorMediaImport))
            .when(!busy, |button| {
                button.on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.import_media(cx)))
            });
        let header = v_flex()
            .gap_1p5()
            .p_2()
            .border_b_1()
            .border_color(color(HAIRLINE))
            .child(h_flex().child(import))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(color(TEXT_3))
                    .child(tr(bardo, Text::EditorMediaImportHint)),
            )
            .when(self.importing > 0, |header| {
                let importing = if self.importing == 1 {
                    bardo.text(Text::EditorMediaImportingOne).into_owned()
                } else {
                    bardo.text_with(
                        Text::EditorMediaImporting,
                        &[("count", &self.importing.to_string())],
                    )
                };
                header.child(label(importing, ACCENT))
            })
            .children(self.import_errors.iter().map(|(file, reason)| {
                div()
                    .text_size(px(11.))
                    .text_color(color(ERROR))
                    .child(SharedString::from(bardo.text_with(
                        Text::EditorMediaImportFailed,
                        &[("file", file), ("reason", &bardo.text(*reason))],
                    )))
            }));
        let media = view.map_or(&[][..], |view| view.media.as_slice());
        let rows: Vec<AnyElement> = media
            .iter()
            .enumerate()
            .map(|(row, media)| {
                let asset = &media.asset;
                let audio = asset.kind == MediaKind::Audio;
                let kind = match asset.picture {
                    Some(picture) => format!(
                        "{} · {}×{}",
                        bardo.text(Text::EditorMediaVideo),
                        picture.width,
                        picture.height
                    ),
                    None => bardo.text(Text::EditorMediaAudio).into_owned(),
                };
                let state = match &media.media {
                    ClipMedia::Ready => None,
                    ClipMedia::Building => Some((Text::EditorProxyBuilding, TEXT_2)),
                    ClipMedia::Missing => Some((Text::EditorMediaMissing, ERROR)),
                    ClipMedia::ProxyFailed(_) => Some((Text::EditorProxyFailed, ERROR)),
                    ClipMedia::ProxyCancelled => Some((Text::EditorProxyCancelled, ERROR)),
                };
                let id = asset.id;
                let place = |button: &'static str, text: Text, track: Track| {
                    tool_button((button, row), editing, false)
                        .h(px(22.))
                        .border_1()
                        .border_color(color(HAIRLINE))
                        .child(tr(bardo, text))
                        .when(editing, |button| {
                            button.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.edit(EditAction::Place { asset: id, track }, cx);
                            }))
                        })
                };
                // One button per track the domain lets this kind of media on.
                let actions = h_flex()
                    .gap_1()
                    .children(asset.kind.tracks().iter().filter_map(|&track| {
                        let (button, text) = match track {
                            Track::Music => ("media-music", Text::EditorMediaAddMusic),
                            Track::Sfx => ("media-sfx", Text::EditorMediaAddSfx),
                            Track::Video => ("media-video", Text::EditorMediaAddVideo),
                            Track::Narration | Track::Captions => return None,
                        };
                        Some(place(button, text, track))
                    }));
                v_flex()
                    .id(("bin-media", row))
                    .gap_1()
                    .p_2()
                    .rounded(px(4.))
                    .hover(|style| style.bg(color(RAISED)))
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .w(px(4.))
                                    .h(px(28.))
                                    .flex_none()
                                    .rounded(px(2.))
                                    .bg(color(if audio { MUSIC } else { VIDEO_EDGE })),
                            )
                            .child(
                                v_flex()
                                    .min_w_0()
                                    .flex_1()
                                    .child(
                                        label(asset.name.clone(), TEXT)
                                            .overflow_hidden()
                                            .text_ellipsis(),
                                    )
                                    .child(label(kind, TEXT_3).text_size(px(11.))),
                            )
                            .child(
                                label(short_duration(asset.duration), TEXT_3)
                                    .font_family(mono.clone()),
                            ),
                    )
                    .children(
                        state.map(|(text, tint)| label(tr(bardo, text), tint).text_size(px(11.))),
                    )
                    .child(actions)
                    .into_any_element()
            })
            .collect();
        let empty = rows.is_empty().then(|| {
            div()
                .p_2()
                .text_size(px(12.))
                .text_color(color(TEXT_3))
                .child(tr(bardo, Text::EditorMediaEmpty))
        });
        v_flex()
            .w(px(260.))
            .flex_none()
            .h_full()
            .bg(color(PANEL))
            .border_r_1()
            .border_color(color(HAIRLINE))
            .child(tabs)
            .child(header)
            .child(
                v_flex()
                    .id("media-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_2()
                    .gap_1()
                    .children(empty)
                    .children(rows)
                    .when(!media.is_empty(), |list| {
                        list.child(
                            div()
                                .px_2()
                                .text_size(px(11.))
                                .text_color(color(TEXT_3))
                                .child(tr(bardo, Text::EditorMediaAddHint)),
                        )
                    }),
            )
            .into_any_element()
    }

    fn render_preview(&self, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let mono = cx.theme().mono_font_family.clone();
        let editor = self.editor.as_ref();
        let aspect = editor.map_or(AspectRatio::Landscape, Editor::aspect);
        let has_cut = editor.is_some_and(|editor| editor.view().timeline.is_some());
        let aspects = h_flex()
            .relative()
            .gap_0p5()
            .p_0p5()
            .rounded(px(4.))
            .bg(color(APP))
            .children(
                [AspectRatio::Landscape, AspectRatio::Vertical]
                    .into_iter()
                    .enumerate()
                    .map(|(position, option)| {
                        tool_button(("aspect", position), has_cut, option == aspect)
                            .h(px(24.))
                            .child(tr(bardo, Text::AspectRatioName(option)))
                            .when(has_cut && option != aspect, |button| {
                                button.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.edit(EditAction::SetAspect(option), cx);
                                }))
                            })
                    }),
            )
            .tour_anchor(
                TourAnchor::Control(Control::EditorAspect),
                Side::Below,
                None,
            );
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

        let overlay = editor.and_then(|editor| self.preview_overlay(editor, cx));
        let picture = self.frame.clone().filter(|_| {
            editor.is_some_and(|editor| !editor.view().is_empty() && editor.can_play())
        });
        let framed = editor.and_then(|editor| self.framed_window(editor));
        let ratio = match (aspect, &framed) {
            (AspectRatio::Vertical, None) => 9. / 16.,
            _ => 16. / 9.,
        };
        // The frame takes the stage's height at the preview's shape; on a
        // narrow stage it is clipped to the width and the picture letterboxed.
        let frame = div()
            .relative()
            .h_full()
            .max_w_full()
            .aspect_ratio(ratio)
            .overflow_hidden()
            .bg(gpui_kit::black())
            .border_1()
            .border_color(color(HAIRLINE));
        let frame = match framed {
            Some(framed) => self.render_framing(frame, framed, picture, bardo, cx),
            None => frame.children(picture.map(|image| {
                img(ImageSource::Render(image))
                    .absolute()
                    .inset_0()
                    .size_full()
                    .object_fit(ObjectFit::Contain)
            })),
        }
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
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(color(PANEL))
            .child(header)
            .child(stage)
            .child(transport)
            .tour_anchor(
                TourAnchor::Control(Control::EditorPreview),
                Side::Right,
                None,
            )
            .into_any_element()
    }

    /// The clip whose crop window the preview shows over its picture, the
    /// picture, and where the window is (where a drag has it, while one
    /// goes on).
    fn framed_window(&self, editor: &Editor) -> Option<FramedWindow> {
        let index = editor.framed_clip()?;
        let item = editor.view().timeline.as_ref()?.video().get(index)?;
        let thumbnail = editor.view().clips.get(index)?.thumbnail.clone()?;
        let Framing::Crop(position) = item.framing else {
            return None;
        };
        let drag = self.crop_drag.filter(|drag| drag.index == index);
        Some(FramedWindow {
            index,
            thumbnail,
            position: drag.map_or(position, |drag| drag.position),
            dragging: drag.is_some(),
        })
    }

    /// A 9:16 crop window over its 16:9 picture: the picture dimmed, the
    /// window showing the preview (or, while dragged, the picture under
    /// it), outlined, and draggable across.
    fn render_framing(
        &self,
        frame: gpui_kit::Div,
        framed: FramedWindow,
        picture: Option<Arc<RenderImage>>,
        bardo: &Bardo,
        cx: &Context<Self>,
    ) -> gpui_kit::Div {
        let window = crop_window(SOURCE, AspectRatio::Vertical, framed.position);
        let across = |pixels: u32| pixels as f32 / SOURCE.width as f32;
        let (left, width) = (across(window.x), across(window.width));
        let backdrop = || {
            img(framed.thumbnail.clone())
                .absolute()
                .top_0()
                .h_full()
                .object_fit(ObjectFit::Cover)
        };
        let content = if framed.dragging || picture.is_none() {
            // The window's share of the picture behind it.
            backdrop()
                .left(relative(-left / width))
                .w(relative(1. / width))
                .into_any_element()
        } else {
            picture
                .map(|image| {
                    img(ImageSource::Render(image))
                        .absolute()
                        .inset_0()
                        .size_full()
                        .object_fit(ObjectFit::Cover)
                        .into_any_element()
                })
                .unwrap_or_else(|| div().into_any_element())
        };
        let bounds = self.framing_box.clone();
        let measure = canvas(move |measured, _, _| bounds.set(measured), |_, _, _, _| {})
            .absolute()
            .inset_0();
        let index = framed.index;
        let start = framed.position;
        frame
            .child(measure)
            .child(backdrop().left_0().w_full().opacity(0.32))
            .child(
                div()
                    .absolute()
                    .top_0()
                    .h_full()
                    .left(relative(left))
                    .w(relative(width))
                    .overflow_hidden()
                    .border_1()
                    .border_color(color(TEXT))
                    .cursor_ew_resize()
                    .child(content)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.crop_drag = Some(CropDrag {
                                index,
                                from_x: event.position.x.into(),
                                start,
                                position: start,
                            });
                            cx.notify();
                        }),
                    ),
            )
            // Over the window, which can slide under it.
            .child(
                label(tr(bardo, Text::EditorFramingSource), TEXT_2)
                    .absolute()
                    .bottom_2()
                    .left_2()
                    .px_1p5()
                    .py_0p5()
                    .rounded(px(3.))
                    .bg(color(APP).opacity(0.8))
                    .text_size(px(10.)),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                let width: f32 = this.framing_box.get().size.width.into();
                if event.pressed_button != Some(MouseButton::Left) {
                    // The button came up where this view did not see it.
                    this.crop_drag = None;
                    return;
                }
                let Some(drag) = this.crop_drag.as_mut() else {
                    return;
                };
                if width <= 0. {
                    return;
                }
                let x: f32 = event.position.x.into();
                let dx = f64::from((x - drag.from_x) / width) * f64::from(SOURCE.width);
                drag.position = drag.start.dragged(SOURCE, AspectRatio::Vertical, dx, 0.0);
                cx.notify();
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| this.drop_crop(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| this.drop_crop(cx)),
            )
    }

    /// Ends a crop drag with the edit it shows.
    fn drop_crop(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.crop_drag.take() else {
            return;
        };
        cx.notify();
        if drag.position != drag.start {
            self.edit(
                EditAction::SetFraming {
                    index: drag.index,
                    framing: Framing::Crop(drag.position),
                },
                cx,
            );
        }
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
        let selection = self.editor.as_ref().and_then(Editor::selection);
        let selected = self.editor.as_ref().and_then(|editor| {
            editor
                .selected_clip()
                .map(|index| editor.view().clips[index].clone())
        });
        let field = |name: Text, value: String| {
            h_flex()
                .justify_between()
                .gap_2()
                .child(label(tr(bardo, name), TEXT_3))
                .child(label(value, TEXT).font_family(mono.clone()))
        };
        let remove = || {
            tool_button("inspector-remove", true, false)
                .border_1()
                .border_color(color(OUTLINE))
                .child(icon(IconName::Delete, TEXT_2))
                .child(tr(bardo, Text::EditorRemove))
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.edit(EditAction::DeleteSelection, cx);
                }))
        };
        let shortcuts = || {
            div()
                .text_size(px(11.))
                .text_color(color(TEXT_3))
                .child(tr(bardo, Text::EditorShortcuts))
        };
        let audio = selection
            .filter(|item| item.track.is_audio())
            .and_then(|item| {
                let editor = self.editor.as_ref()?;
                let piece = editor
                    .view()
                    .timeline
                    .as_ref()?
                    .audio(item.track.lane()?)
                    .get(item.index)?
                    .clone();
                Some(piece)
            });
        // The imported file an audio item plays, by the name the user knows.
        let audio_name = audio.as_ref().and_then(|piece| {
            let editor = self.editor.as_ref()?;
            Some(editor.view().media_file(&piece.file)?.asset.name.clone())
        });
        let audio_track =
            selection
                .filter(|item| item.track.is_audio())
                .map(|item| match item.track {
                    Track::Music => Text::EditorTrackMusic,
                    Track::Sfx => Text::EditorTrackSfx,
                    _ => Text::EditorTrackNarration,
                });
        let lane = self.editor.as_ref().and_then(|editor| {
            Some((
                editor.selected_lane()?,
                *editor.view().timeline.as_ref()?.mix(),
            ))
        });
        let section = |text: Text| label(tr(bardo, text), TEXT_3);
        let fades = selection
            .filter(|item| item.track.is_audio())
            .zip(audio.as_ref())
            .map(|(item, piece)| {
                let (fade_in, fade_out) = piece.fades();
                let set = |fade_in: Duration, fade_out: Duration| EditAction::SetFades {
                    item,
                    fade_in,
                    fade_out,
                };
                let room = piece.duration.saturating_sub(fade_in + fade_out);
                v_flex()
                    .gap_1p5()
                    .child(section(Text::EditorFades))
                    .child(
                        self.stepper(
                            "fade-in",
                            tr(bardo, Text::EditorFadeIn),
                            short_duration(fade_in),
                            (!fade_in.is_zero())
                                .then(|| set(fade_in.saturating_sub(FADE_STEP), fade_out)),
                            (!room.is_zero()).then(|| set(fade_in + FADE_STEP.min(room), fade_out)),
                            cx,
                        ),
                    )
                    .child(
                        self.stepper(
                            "fade-out",
                            tr(bardo, Text::EditorFadeOut),
                            short_duration(fade_out),
                            (!fade_out.is_zero())
                                .then(|| set(fade_in, fade_out.saturating_sub(FADE_STEP))),
                            (!room.is_zero()).then(|| set(fade_in, fade_out + FADE_STEP.min(room))),
                            cx,
                        ),
                    )
            });
        let caption = selection.filter(|item| item.track == Track::Captions);
        let body = match (caption, selected, audio, lane) {
            (Some(item), ..) => self.render_caption_inspector(item, cx),
            (None, None, None, Some((lane, mix))) => self.render_lane_inspector(lane, mix, cx),
            (None, None, Some(piece), _) => v_flex()
                .p_3()
                .gap_3()
                .child(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(13.))
                                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                .text_color(color(TEXT))
                                .child(tr(
                                    bardo,
                                    audio_track.unwrap_or(Text::EditorTrackNarration),
                                )),
                        )
                        .children(audio_name.map(|name| label(name, TEXT_2))),
                )
                .child(
                    v_flex()
                        .gap_1p5()
                        .child(field(Text::EditorIn, timecode(piece.at)))
                        .child(field(Text::EditorOut, timecode(piece.end())))
                        .child(field(Text::EditorLength, short_duration(piece.duration)))
                        .child(field(Text::EditorSourceIn, timecode(piece.start))),
                )
                .children(fades)
                .child(
                    v_flex()
                        .gap_1()
                        .child(label(tr(bardo, Text::EditorFile), TEXT_3))
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(color(TEXT_2))
                                .font_family(mono.clone())
                                .child(SharedString::from(piece.file)),
                        ),
                )
                .child(remove())
                .child(shortcuts())
                .into_any_element(),
            (None, None, None, None) => div()
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
            (None, Some(clip), _, _) => {
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
                                    .child(clip_name(bardo, &clip)),
                            )
                            .children(provenance.into_iter().map(|line| {
                                div()
                                    .text_size(px(12.))
                                    .text_color(color(TEXT_2))
                                    .child(SharedString::from(line))
                            })),
                    )
                    .child(
                        v_flex()
                            .gap_1p5()
                            .child(field(Text::EditorIn, timecode(clip.at)))
                            .child(field(Text::EditorOut, timecode(clip.at + clip.duration)))
                            .child(field(Text::EditorLength, short_duration(clip.duration)))
                            .when(clip.is_clip, |fields| {
                                fields.child(field(Text::EditorSourceIn, timecode(clip.start)))
                            }),
                    )
                    .children(
                        self.editor
                            .as_ref()
                            .and_then(Editor::selected_clip)
                            .and_then(|index| self.render_framing_controls(index, cx)),
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
                    .when(clip.shows.scene().is_some(), |body| {
                        body.child(
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
                    })
                    .child(remove())
                    .child(shortcuts())
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
                    .child(label(
                        tr(
                            bardo,
                            if lane.is_some() {
                                Text::EditorInspectorTrack
                            } else if caption.is_some() {
                                Text::EditorInspectorCaption
                            } else if selection.is_some_and(|item| item.track.is_audio()) {
                                Text::EditorInspectorAudio
                            } else {
                                Text::EditorInspectorClip
                            },
                        ),
                        TEXT_2,
                    )),
            )
            .child(body)
            .into_any_element()
    }

    /// A selected caption: its text, where it shows, the project's caption
    /// style and whether captions show at all.
    fn render_caption_inspector(&self, item: ItemRef, cx: &Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let mono = cx.theme().mono_font_family.clone();
        let Some(editor) = self.editor.as_ref() else {
            return div().into_any_element();
        };
        let Some(captions) = editor
            .view()
            .timeline
            .as_ref()
            .map(|timeline| timeline.captions())
        else {
            return div().into_any_element();
        };
        let span = editor.view().span(item);
        let field = |name: Text, value: String| {
            h_flex()
                .justify_between()
                .gap_2()
                .child(label(tr(bardo, name), TEXT_3))
                .child(label(value, TEXT).font_family(mono.clone()))
        };
        let hint = |text: Text| {
            div()
                .text_size(px(11.))
                .text_color(color(TEXT_3))
                .child(tr(bardo, text))
        };
        let section = |text: Text| label(tr(bardo, text), TEXT_3);
        let swatches = h_flex().gap_2().children(CaptionStyle::ALL.map(|style| {
            let look = caption_look(style);
            let picked = captions.style() == style;
            let name = tr(bardo, Text::CaptionStyleName(style));
            let sample = if look.uppercase {
                name.to_uppercase()
            } else {
                name.to_string()
            };
            let text = div()
                .px_1()
                .text_size(px(12.))
                .font_weight(if look.bold || look.uppercase {
                    gpui_kit::FontWeight::BOLD
                } else {
                    gpui_kit::FontWeight::NORMAL
                })
                .text_color(Hsla::from(rgb(look.fill)))
                .when_some(look.band, |text, (band, opacity, _)| {
                    text.bg(Hsla::from(rgb(band)).opacity(opacity))
                })
                .child(sample);
            v_flex()
                .id(("caption-style", style as usize))
                .flex_1()
                .h(px(48.))
                .items_center()
                .justify_center()
                .rounded(px(4.))
                .cursor_pointer()
                .bg(color(VIDEO_FILL))
                .border_1()
                .border_color(color(if picked { ACCENT } else { OUTLINE }))
                .when(picked, |swatch| swatch.border_2())
                .hover(|style| style.border_color(color(TEXT_2)))
                .child(text)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.edit(EditAction::SetCaptionStyle(style), cx);
                }))
        }));
        let remove = tool_button("inspector-remove", true, false)
            .border_1()
            .border_color(color(OUTLINE))
            .child(icon(IconName::Delete, TEXT_2))
            .child(tr(bardo, Text::EditorRemove))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.edit(EditAction::DeleteSelection, cx);
            }));
        v_flex()
            .p_3()
            .gap_3()
            .child(
                div()
                    .text_size(px(13.))
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .text_color(color(TEXT))
                    .child(tr(bardo, Text::EditorInspectorCaption)),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(section(Text::EditorCaptionText))
                    .child(
                        // The field keeps the focus a click gives it.
                        div()
                            .on_mouse_down(gpui_kit::MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation()
                            })
                            .child(
                                // In the editor's dark look, not the app's.
                                Input::new(&self.caption_input)
                                    .small()
                                    .bg(color(APP))
                                    .text_color(color(TEXT))
                                    .border_color(color(OUTLINE)),
                            ),
                    )
                    .child(hint(Text::EditorCaptionTextHint)),
            )
            .children(span.map(|(at, duration)| {
                v_flex()
                    .gap_1p5()
                    .child(field(Text::EditorIn, timecode(at)))
                    .child(field(Text::EditorOut, timecode(at + duration)))
                    .child(field(Text::EditorLength, short_duration(duration)))
            }))
            .child(
                v_flex()
                    .gap_1p5()
                    .child(section(Text::EditorCaptionStyle))
                    .child(swatches)
                    .child(hint(Text::EditorCaptionStyleHint)),
            )
            .child(self.switch_row(
                "captions-shown-row",
                tr(bardo, Text::EditorShowCaptions),
                captions.shown(),
                EditAction::ShowCaptions(!captions.shown()),
                cx,
            ))
            .child(remove)
            .into_any_element()
    }

    /// A labelled value with − and + buttons that make `less` and `more`;
    /// a button with nothing to make is drawn disabled.
    fn stepper(
        &self,
        id: &'static str,
        name: SharedString,
        value: String,
        less: Option<EditAction>,
        more: Option<EditAction>,
        cx: &Context<Self>,
    ) -> AnyElement {
        let mono = cx.theme().mono_font_family.clone();
        let button = |suffix: usize, glyph: IconName, action: Option<EditAction>| {
            tool_button((id, suffix), action.is_some(), false)
                .h(px(24.))
                .min_w(px(24.))
                .px_1()
                .border_1()
                .border_color(color(OUTLINE))
                .child(icon(glyph, TEXT_2).size_3())
                .when_some(action, |button, action| {
                    button.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.edit(action, cx);
                    }))
                })
        };
        h_flex()
            .justify_between()
            .gap_2()
            .child(label(name, TEXT_3))
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(button(0, IconName::Minus, less))
                    .child(
                        label(value, TEXT)
                            .font_family(mono)
                            .min_w(px(64.))
                            .text_center(),
                    )
                    .child(button(1, IconName::Plus, more)),
            )
            .into_any_element()
    }

    /// A selected clip's framing in a 9:16 cut: fit, fill (a centered
    /// window) or a window placed elsewhere, and where across it sits.
    /// Hidden while the clip's media is missing.
    fn render_framing_controls(&self, index: usize, cx: &Context<Self>) -> Option<AnyElement> {
        let bardo = self.bardo.read(cx);
        let editor = self.editor.as_ref()?;
        let item = editor.view().timeline.as_ref()?.video().get(index)?;
        if editor.view().clips.get(index)?.media == ClipMedia::Missing {
            return None;
        }
        let vertical = editor.aspect() == AspectRatio::Vertical;
        let framing = item.framing;
        let set = move |framing: Framing| EditAction::SetFraming { index, framing };
        let custom = matches!(framing, Framing::Crop(position) if position != CropPosition::CENTER);
        let choices = [
            (
                Text::EditorFramingFit,
                Some(Framing::Fit),
                framing == Framing::Fit,
            ),
            (
                Text::EditorFramingFill,
                Some(Framing::FILL),
                framing == Framing::FILL,
            ),
            // Reached by moving the window, not picked.
            (Text::EditorFramingCustom, None, custom),
        ]
        .into_iter()
        .enumerate()
        .map(|(position, (text, target, active))| {
            let target = target.filter(|_| vertical && !active);
            tool_button(
                ("framing", position),
                vertical && (target.is_some() || active),
                active,
            )
            .h(px(24.))
            .flex_1()
            .border_1()
            .border_color(color(OUTLINE))
            .child(tr(bardo, text))
            .when_some(target, |button, target| {
                button.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.edit(set(target), cx);
                }))
            })
        });
        let (value, less, more) = match framing {
            Framing::Crop(position) => (
                format!("{}%", position.x() / 10),
                (vertical && position.x() > 0)
                    .then(|| set(Framing::Crop(position.nudged(-CROP_STEP)))),
                (vertical && position.x() < bardo_app::bardo_domain::CROP_STEPS)
                    .then(|| set(Framing::Crop(position.nudged(CROP_STEP)))),
            ),
            Framing::Fit => ("—".to_owned(), None, None),
        };
        let hint = if !vertical {
            Text::EditorFramingLandscapeHint
        } else if framing == Framing::Fit {
            Text::EditorFramingFitHint
        } else {
            Text::EditorFramingDragHint
        };
        Some(
            v_flex()
                .gap_1p5()
                .child(label(tr(bardo, Text::EditorFraming), TEXT_3))
                .child(h_flex().gap_1().children(choices))
                .child(self.stepper(
                    "crop-x",
                    tr(bardo, Text::EditorFramingPosition),
                    value,
                    less,
                    more,
                    cx,
                ))
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(color(TEXT_3))
                        .child(tr(bardo, hint)),
                )
                .into_any_element(),
        )
    }

    /// A toggle row: a name and an on/off pill that makes `action`.
    fn switch_row(
        &self,
        id: &'static str,
        name: SharedString,
        on: bool,
        action: EditAction,
        cx: &Context<Self>,
    ) -> AnyElement {
        h_flex()
            .id(id)
            .justify_between()
            .gap_2()
            .cursor_pointer()
            .child(label(name, TEXT_2))
            .child(
                div()
                    .w(px(28.))
                    .h(px(16.))
                    .p(px(2.))
                    .rounded_full()
                    .bg(color(if on { TEXT_2 } else { RAISED_HOVER }))
                    .border_1()
                    .border_color(color(OUTLINE))
                    .flex()
                    .when(on, |track| track.justify_end())
                    .child(div().size(px(10.)).rounded_full().bg(color(if on {
                        APP
                    } else {
                        TEXT_3
                    }))),
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.edit(action, cx)))
            .into_any_element()
    }

    /// A picked lane's mix: level, mute and solo, and on the music how deep
    /// it ducks under the narration.
    fn render_lane_inspector(
        &self,
        lane: AudioLane,
        mix: bardo_app::bardo_domain::Mix,
        cx: &Context<Self>,
    ) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let name = match lane {
            AudioLane::Narration => Text::EditorTrackNarration,
            AudioLane::Music => Text::EditorTrackMusic,
            AudioLane::Sfx => Text::EditorTrackSfx,
        };
        let current = mix.lane(lane);
        let set = |mix: LaneMix| EditAction::SetLane { lane, mix };
        let gain = |by: i16| {
            let gain = current
                .gain
                .nudged(Decibels::from_tenths(by * GAIN_STEP.tenths()), GAIN_RANGE);
            (gain != current.gain).then(|| set(LaneMix { gain, ..current }))
        };
        let title = div()
            .text_size(px(13.))
            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
            .text_color(color(TEXT))
            .child(tr(bardo, name));
        let hint = |text: Text| {
            div()
                .text_size(px(11.))
                .text_color(color(TEXT_3))
                .child(tr(bardo, text))
        };
        let level = self.stepper(
            "lane-gain",
            tr(bardo, Text::EditorLevel),
            bardo.decibels(current.gain),
            gain(-1),
            gain(1),
            cx,
        );
        let mute = self.switch_row(
            "lane-mute-row",
            tr(bardo, Text::EditorMute),
            current.muted,
            set(LaneMix {
                muted: !current.muted,
                ..current
            }),
            cx,
        );
        let solo = self.switch_row(
            "lane-solo-row",
            tr(bardo, Text::EditorSolo),
            current.solo,
            set(LaneMix {
                solo: !current.solo,
                ..current
            }),
            cx,
        );
        let ducking = (lane == AudioLane::Music).then(|| {
            let ducking = mix.ducking;
            let depth = |by: i16| {
                let depth = ducking
                    .depth
                    .nudged(Decibels::from_tenths(by * DUCK_STEP.tenths()), DUCK_RANGE);
                (ducking.on && depth != ducking.depth)
                    .then_some(EditAction::SetDucking(Ducking { depth, ..ducking }))
            };
            v_flex()
                .gap_1p5()
                .pt_2()
                .border_t_1()
                .border_color(color(HAIRLINE))
                .child(self.switch_row(
                    "lane-duck-row",
                    tr(bardo, Text::EditorDuck),
                    ducking.on,
                    EditAction::SetDucking(Ducking {
                        on: !ducking.on,
                        ..ducking
                    }),
                    cx,
                ))
                .child(self.stepper(
                    "duck-depth",
                    tr(bardo, Text::EditorDuckDepth),
                    bardo.decibels(Decibels::from_tenths(-ducking.depth.tenths())),
                    // The readout is signed, so − ducks deeper.
                    depth(1),
                    depth(-1),
                    cx,
                ))
                .child(hint(Text::EditorDuckHint))
        });
        v_flex()
            .p_3()
            .gap_3()
            .child(title)
            .child(v_flex().gap_1p5().child(level).child(mute).child(solo))
            .children(ducking)
            .child(hint(Text::EditorLaneHint))
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
        let cut_reset = self.editor.as_ref().is_some_and(Editor::cut_reset);
        if problems.is_empty() && !view.stale && error.is_none() && !cut_reset {
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
            .chain(cut_reset.then_some(Text::EditorCutReset))
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
                h_flex()
                    .mt_2()
                    .gap_2()
                    .child(
                        tool_button("empty-back", true, true)
                            .child(tr(bardo, Text::EditorBackToProject))
                            .on_click(
                                cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(EditorEvent::Close)),
                            ),
                    )
                    // Media can come in before the cut: it waits in the bin.
                    .child(
                        tool_button("empty-import", true, false)
                            .border_1()
                            .border_color(color(HAIRLINE))
                            .child(tr(bardo, Text::EditorMediaImport))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.bin_tab = BinTab::Media;
                                this.import_media(cx);
                            })),
                    ),
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
    if let ClipShows::Footage(_) = clip.shows {
        lines.push(bardo.text(Text::EditorSourceFootage).into_owned());
        return lines;
    }
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
    let file = problem.file.as_deref().unwrap_or("—");
    match &problem.shows {
        ClipShows::Scene(scene) => bardo.text_with(
            Text::EditorProblemLine,
            &[
                ("n", &(scene + 1).to_string()),
                ("file", file),
                ("reason", &reason),
            ],
        ),
        ClipShows::Footage(name) => bardo.text_with(
            Text::EditorProblemFootageLine,
            &[("name", name), ("file", file), ("reason", &reason)],
        ),
    }
}

/// A clip's name: its scene, or the imported file's name.
pub(crate) fn clip_name(bardo: &Bardo, clip: &ClipView) -> String {
    match &clip.shows {
        ClipShows::Scene(scene) => {
            bardo.text_with(Text::EditorSceneLabel, &[("n", &(scene + 1).to_string())])
        }
        ClipShows::Footage(name) => name.clone(),
    }
}

impl Render for EditorScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.advance(window, cx);
        self.sync_caption_input(window, cx);
        // A drag whose clip left the preview (playback moved on, the clip
        // changed) never sees its mouse-up: drop it without an edit.
        if let Some(drag) = self.crop_drag
            && self.editor.as_ref().and_then(Editor::framed_clip) != Some(drag.index)
        {
            self.crop_drag = None;
        }
        let view = self.editor.as_ref().map(|editor| editor.view().clone());
        let top_bar = self.render_top_bar(view.as_ref(), cx);
        let root =
            v_flex()
                .id("editor")
                .track_focus(&self.focus)
                .size_full()
                .bg(color(APP))
                .text_color(color(TEXT))
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    this.on_key(event, window, cx)
                }))
                .when(self.cuts.on, |root| root.key_context(suggestions::CONTEXT))
                .on_action(cx.listener(|this, _: &suggestions::NextCut, _, cx| this.next_cut(cx)))
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
            .child(if self.cuts.on && view.timeline.is_some() {
                self.render_cuts_panel(cx)
            } else {
                self.render_inspector(cx)
            });
        let banners = self.render_banners(&view, cx);
        let lower = if view.timeline.is_none() {
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
