//! Who appears (#337), on a video's Scenes stage: the world's characters
//! the video shows, each with an instruction for this video, and the ones
//! it may still pick. The scene planner gets the picked ones as the fixed
//! cast. Rules and storage live in `bardo_app`; this file maps them to
//! the screen. Nothing here is paid.

use bardo_app::bardo_domain::{CastPick, Character, CharacterId, MAX_PICKED, VideoProjectId};
use bardo_app::{Bardo, Control, Text, TourAnchor, WhoAppears};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Entity, EventEmitter, SharedString, Subscription, Window, div,
};

use crate::guide;
use crate::kit::{self, Tone};
use crate::shell::tr;

/// Who appears changed: the stage reads the plan again.
pub struct WhoAppearsChanged;

/// What the panel says after the last action.
#[derive(Clone, Copy)]
enum Notice {
    Saved,
    Error(Text),
}

/// A picked character and the input holding its instruction.
struct Picked {
    character: Character,
    instruction: Entity<InputState>,
    /// The instruction as saved.
    saved: String,
}

pub struct WhoAppearsPanel {
    bardo: Entity<Bardo>,
    video: Option<VideoProjectId>,
    picked: Vec<Picked>,
    available: Vec<Character>,
    changed_after_plan: bool,
    notice: Option<Notice>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<WhoAppearsChanged> for WhoAppearsPanel {}

impl WhoAppearsPanel {
    pub fn new(bardo: Entity<Bardo>) -> Self {
        Self {
            bardo,
            video: None,
            picked: Vec::new(),
            available: Vec::new(),
            changed_after_plan: false,
            notice: None,
            _subscriptions: Vec::new(),
        }
    }

    /// Shows who appears in `video` as it is stored now.
    pub fn show(
        &mut self,
        video: Option<VideoProjectId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.video != video {
            self.notice = None;
        }
        self.video = video;
        let read = video.map(|video| self.bardo.read(cx).who_appears(video));
        match read {
            Some(Ok(shown)) => self.fill(shown, window, cx),
            Some(Err(_)) | None => {
                self.picked.clear();
                self.available.clear();
                self.changed_after_plan = false;
                self._subscriptions.clear();
            }
        }
        cx.notify();
    }

    fn fill(&mut self, shown: WhoAppears, window: &mut Window, cx: &mut Context<Self>) {
        let placeholder = tr(self.bardo.read(cx), Text::WhoAppearsInstructionPlaceholder);
        // An instruction being typed stays as it is.
        let mut old: Vec<Picked> = std::mem::take(&mut self.picked);
        let mut subscriptions = Vec::new();
        for (character, instruction) in shown.picked {
            let kept = old
                .iter()
                .position(|p| p.character.id == character.id)
                .map(|ix| old.swap_remove(ix));
            let picked = match kept {
                Some(mut kept) if kept.saved == instruction => {
                    kept.character = character;
                    kept
                }
                _ => {
                    let value = instruction.clone();
                    let placeholder = placeholder.clone();
                    let input = cx.new(|cx| {
                        let mut input = InputState::new(window, cx).placeholder(placeholder);
                        input.set_value(value, window, cx);
                        input
                    });
                    Picked {
                        character,
                        instruction: input,
                        saved: instruction,
                    }
                }
            };
            subscriptions.push(cx.subscribe(
                &picked.instruction,
                |this, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.notice = None;
                        cx.notify();
                    }
                },
            ));
            self.picked.push(picked);
        }
        self._subscriptions = subscriptions;
        let picked: Vec<CharacterId> = self.picked.iter().map(|p| p.character.id).collect();
        self.available = shown
            .available
            .into_iter()
            .filter(|character| !picked.contains(&character.id))
            .collect();
        self.changed_after_plan = shown.changed_after_plan;
    }

    fn picks(&self, cx: &App) -> Vec<CastPick> {
        self.picked
            .iter()
            .map(|picked| CastPick {
                character: picked.character.id,
                instruction: picked.instruction.read(cx).value().to_string(),
            })
            .collect()
    }

    fn dirty(&self, cx: &App) -> bool {
        self.picked
            .iter()
            .any(|picked| picked.instruction.read(cx).value().trim() != picked.saved)
    }

    fn set(&mut self, picks: Vec<CastPick>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(video) = self.video else {
            return;
        };
        match self.bardo.read(cx).set_who_appears(video, picks) {
            Ok(()) => {
                self.notice = Some(Notice::Saved);
                cx.emit(WhoAppearsChanged);
            }
            Err(error) => self.notice = Some(Notice::Error(error.message())),
        }
        let notice = self.notice;
        self.show(Some(video), window, cx);
        self.notice = notice;
        cx.notify();
    }

    fn add(&mut self, character: CharacterId, window: &mut Window, cx: &mut Context<Self>) {
        let mut picks = self.picks(cx);
        picks.push(CastPick {
            character,
            instruction: String::new(),
        });
        self.set(picks, window, cx);
    }

    fn remove(&mut self, character: CharacterId, window: &mut Window, cx: &mut Context<Self>) {
        let mut picks = self.picks(cx);
        picks.retain(|pick| pick.character != character);
        self.set(picks, window, cx);
    }

    /// Whether the open video has anything to show here.
    fn shows(&self) -> bool {
        self.video.is_some()
    }

    fn owner_tag(bardo: &Bardo, character: &Character, cx: &App) -> AnyElement {
        let text = match character.owner_project {
            Some(_) => Text::WorldOwnerProject,
            None => Text::WorldOwnerGlobal,
        };
        kit::status(Tone::Neutral, tr(bardo, text), cx).into_any_element()
    }

    fn render_picked(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let picked = &self.picked[ix];
        let bardo = self.bardo.read(cx);
        let id = picked.character.id;
        kit::well(cx)
            .p_2()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .flex_wrap()
                    .child(
                        div()
                            .font_medium()
                            .child(SharedString::from(picked.character.name().to_owned())),
                    )
                    .child(Self::owner_tag(bardo, &picked.character, cx))
                    .child(div().flex_1())
                    .child(
                        Button::new(("who-appears-remove", ix))
                            .xsmall()
                            .ghost()
                            .label(tr(bardo, Text::WhoAppearsRemove))
                            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                this.remove(id, window, cx)
                            })),
                    ),
            )
            .child(kit::field(
                tr(bardo, Text::WhoAppearsInstruction),
                None,
                Input::new(&picked.instruction).small().into_any_element(),
                None,
            ))
            .into_any_element()
    }
}

impl Render for WhoAppearsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.shows() {
            return div().into_any_element();
        }
        let rows: Vec<AnyElement> = (0..self.picked.len())
            .map(|ix| self.render_picked(ix, cx))
            .collect();
        let full = self.picked.len() >= MAX_PICKED;
        let dirty = self.dirty(cx);
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let available: Vec<AnyElement> =
            self.available
                .iter()
                .enumerate()
                .map(|(ix, character)| {
                    let id = character.id;
                    Button::new(("who-appears-add", ix))
                        .xsmall()
                        .outline()
                        .label(SharedString::from(format!("+ {}", character.name())))
                        .tooltip(tr(
                            bardo,
                            match character.owner_project {
                                Some(_) => Text::WorldOwnerProject,
                                None => Text::WorldOwnerGlobal,
                            },
                        ))
                        .disabled(full)
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.add(id, window, cx)
                        }))
                        .into_any_element()
                })
                .collect();
        let notice = self.notice.map(|notice| match notice {
            Notice::Saved => kit::notice(Tone::Success, tr(bardo, Text::WhoAppearsSaved), cx),
            Notice::Error(text) => kit::notice(Tone::Danger, tr(bardo, text), cx),
        });
        let card = kit::card(cx)
            .p_3()
            .gap_2()
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(kit::section_heading(tr(bardo, Text::WhoAppearsTitle)))
                    .child(guide::info(
                        bardo,
                        "who-appears-info",
                        tr(bardo, Text::WhoAppearsHint),
                        guide::refs::SCENES_WHO_APPEARS,
                    )),
            )
            .when(self.changed_after_plan, |card| {
                card.child(kit::notice(
                    Tone::Warning,
                    tr(bardo, Text::WhoAppearsChanged),
                    cx,
                ))
            })
            .map(|card| {
                if rows.is_empty() {
                    card.child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(tr(bardo, Text::WhoAppearsNone)),
                    )
                } else {
                    card.child(v_flex().gap_2().children(rows))
                }
            })
            .when(dirty, |card| {
                card.child(
                    h_flex().child(
                        Button::new("who-appears-save")
                            .small()
                            .primary()
                            .label(tr(bardo, Text::WhoAppearsSave))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                let picks = this.picks(cx);
                                this.set(picks, window, cx);
                            })),
                    ),
                )
            })
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .font_medium()
                            .text_color(theme.muted_foreground)
                            .child(tr(
                                bardo,
                                if full {
                                    Text::WhoAppearsFull
                                } else {
                                    Text::WhoAppearsPickable
                                },
                            )),
                    )
                    .map(|col| {
                        if available.is_empty() {
                            col.child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(tr(bardo, Text::WhoAppearsNoneAvailable)),
                            )
                        } else {
                            col.child(h_flex().flex_wrap().gap_1().children(available))
                        }
                    }),
            )
            .children(notice);
        kit::anchor(TourAnchor::Control(Control::WhoAppears), card).into_any_element()
    }
}
