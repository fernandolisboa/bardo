//! Loose videos on the Videos screen (#341): with *No project* open, the
//! screen lists the videos that have no project and no channel. *New loose
//! video* types an idea (title and angle) and picks the channel whose
//! settings it starts with, or a new channel's defaults; *Move to a
//! project…* picks an active project's channel, says first where the video
//! differs from the channel's audience, then moves it. The rules live in
//! `bardo_app` (`start_loose_video`, `loose_move_differences`,
//! `move_loose_video`).

use bardo_app::bardo_domain::{ChannelId, SettingDifference, VideoProjectId};
use bardo_app::{Control, LooseVideoDraft, Text, TourAnchor};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::searchable_list::SearchableVec;
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::{Disableable as _, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, ClickEvent, Entity, SharedString, Subscription, Window, div};

use super::{Choice, ChoiceSelect, ProjectsScreen};
use crate::kit::{self, Tone};
use crate::shell::tr;

/// *New loose video*, while open.
pub(super) struct LooseForm {
    title: Entity<InputState>,
    angle: Entity<InputState>,
    copy_from: ChoiceSelect<Option<ChannelId>>,
    error: Option<Text>,
    _subscriptions: Vec<Subscription>,
}

/// *Move to a project*, while open.
pub(super) struct MoveForm {
    video: VideoProjectId,
    channel: ChoiceSelect<ChannelId>,
    /// The channel picked, and where the video differs from it.
    picked: Option<(ChannelId, Vec<SettingDifference>)>,
    /// No active project has a channel.
    none: bool,
    error: Option<Text>,
    _subscription: Subscription,
}

impl ProjectsScreen {
    /// Opens *New loose video*.
    pub(super) fn start_loose_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let bardo = self.bardo.read(cx);
        let title_placeholder = tr(bardo, Text::LooseIdeaTitlePlaceholder);
        let angle_placeholder = tr(bardo, Text::LooseIdeaAnglePlaceholder);
        let choices = SearchableVec::new(
            std::iter::once(Choice {
                value: None,
                title: tr(bardo, Text::LooseCopyDefaults),
            })
            .chain(
                bardo
                    .loose_channel_choices()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(id, name)| Choice {
                        value: Some(id),
                        title: SharedString::from(name),
                    }),
            )
            .collect::<Vec<_>>(),
        );
        let title = cx.new(|cx| InputState::new(window, cx).placeholder(title_placeholder));
        let angle = cx.new(|cx| InputState::new(window, cx).placeholder(angle_placeholder));
        let copy_from = cx.new(|cx| {
            let mut select = SelectState::new(choices, None, window, cx).searchable(true);
            select.set_selected_value(&None, window, cx);
            select
        });
        let mut subscriptions = Vec::new();
        for input in [&title, &angle] {
            subscriptions.push(cx.subscribe_in(
                input,
                window,
                |this, _, event: &InputEvent, window, cx| match event {
                    InputEvent::PressEnter { .. } => this.create_loose(window, cx),
                    InputEvent::Change => {
                        if let Some(form) = &mut this.loose_form {
                            form.error = None;
                        }
                        cx.notify();
                    }
                    _ => {}
                },
            ));
        }
        title.update(cx, |title, cx| title.focus(window, cx));
        self.move_form = None;
        self.loose_form = Some(LooseForm {
            title,
            angle,
            copy_from,
            error: None,
            _subscriptions: subscriptions,
        });
        cx.notify();
    }

    fn create_loose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &self.loose_form else {
            return;
        };
        let draft = LooseVideoDraft {
            title: form.title.read(cx).value().to_string(),
            angle: form.angle.read(cx).value().to_string(),
            copy_from: form.copy_from.read(cx).selected_value().copied().flatten(),
        };
        let started = self.bardo.update(cx, |bardo, cx| {
            let started = bardo.start_loose_video(draft);
            cx.notify();
            started
        });
        match started {
            Ok(video) => {
                self.loose_form = None;
                self.show_archived = false;
                self.load_projects(cx);
                self.select_project(Some(video.id), window, cx);
            }
            Err(error) => {
                if let Some(form) = &mut self.loose_form {
                    form.error = Some(error.message());
                }
                cx.notify();
            }
        }
    }

    /// Opens *Move to a project* for the open loose video.
    pub(super) fn start_move(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(video) = self.project else {
            return;
        };
        let bardo = self.bardo.read(cx);
        let found = bardo.loose_channel_choices().unwrap_or_default();
        let none = found.is_empty();
        let choices = SearchableVec::new(
            found
                .into_iter()
                .map(|(id, name)| Choice {
                    value: id,
                    title: SharedString::from(name),
                })
                .collect::<Vec<_>>(),
        );
        let channel = cx.new(|cx| SelectState::new(choices, None, window, cx).searchable(true));
        let subscription = cx.subscribe_in(
            &channel,
            window,
            |this, _, event: &SelectEvent<SearchableVec<Choice<ChannelId>>>, _, cx| {
                let SelectEvent::Confirm(Some(id)) = event else {
                    return;
                };
                this.pick_move_channel(*id, cx);
            },
        );
        self.loose_form = None;
        self.move_form = Some(MoveForm {
            video,
            channel,
            picked: None,
            none,
            error: None,
            _subscription: subscription,
        });
        cx.notify();
    }

    fn pick_move_channel(&mut self, channel: ChannelId, cx: &mut Context<Self>) {
        let Some(form) = &mut self.move_form else {
            return;
        };
        match self
            .bardo
            .read(cx)
            .loose_move_differences(form.video, channel)
        {
            Ok(differences) => {
                form.picked = Some((channel, differences));
                form.error = None;
            }
            Err(error) => {
                form.picked = None;
                form.error = Some(error.message());
            }
        }
        cx.notify();
    }

    fn confirm_move(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((video, channel)) = self
            .move_form
            .as_ref()
            .and_then(|form| Some((form.video, form.picked.as_ref()?.0)))
        else {
            return;
        };
        let moved = self.bardo.update(cx, |bardo, cx| {
            let moved = bardo.move_loose_video(video, channel);
            cx.notify();
            moved
        });
        match moved {
            Ok(()) => {
                self.move_form = None;
                self.load_projects(cx);
                let next = self.projects.first().map(|project| project.id);
                self.select_project(next, window, cx);
                self.notice = Some(Text::LooseMoved);
                cx.notify();
            }
            Err(error) => {
                if let Some(form) = &mut self.move_form {
                    form.error = Some(error.message());
                }
                cx.notify();
            }
        }
    }

    fn close_loose_forms(&mut self, cx: &mut Context<Self>) {
        self.loose_form = None;
        self.move_form = None;
        cx.notify();
    }

    /// *New loose video* as a header action.
    pub(super) fn new_loose_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        kit::anchor(
            TourAnchor::Control(Control::LooseNew),
            Button::new("loose-new")
                .small()
                .icon(IconName::Plus)
                .label(tr(bardo, Text::LooseNew))
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.start_loose_form(window, cx);
                })),
        )
        .into_any_element()
    }

    /// *Move to a project…* as a header action, for a loose video that is
    /// not archived.
    pub(super) fn move_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        Button::new("loose-move")
            .ghost()
            .small()
            .icon(IconName::ArrowRight)
            .label(tr(bardo, Text::LooseMove))
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                this.start_move(window, cx);
            }))
            .into_any_element()
    }

    /// The open form, as a card over the stage.
    pub(super) fn loose_card(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if let Some(form) = &self.loose_form {
            return Some(self.render_loose_form(form, cx));
        }
        self.move_form
            .as_ref()
            .map(|form| self.render_move_form(form, cx))
    }

    fn render_loose_form(&self, form: &LooseForm, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let error = form
            .error
            .map(|error| kit::notice(Tone::Danger, tr(bardo, error), cx).into_any_element());
        kit::card(cx)
            .p_4()
            .child(
                v_flex()
                    .gap_3()
                    .child(kit::section_heading(tr(bardo, Text::LooseFormTitle)))
                    .child(muted_line(cx, tr(bardo, Text::LooseFormHint)))
                    .child(kit::field(
                        tr(bardo, Text::LooseIdeaTitle),
                        None,
                        Input::new(&form.title).small().into_any_element(),
                        None,
                    ))
                    .child(kit::field(
                        tr(bardo, Text::LooseIdeaAngle),
                        None,
                        Input::new(&form.angle).small().into_any_element(),
                        None,
                    ))
                    .child(kit::field(
                        tr(bardo, Text::LooseCopyFrom),
                        None,
                        Select::new(&form.copy_from).small().into_any_element(),
                        None,
                    ))
                    .children(error)
                    .child(
                        h_flex()
                            .gap_2()
                            .justify_end()
                            .child(
                                Button::new("loose-cancel")
                                    .ghost()
                                    .small()
                                    .label(tr(bardo, Text::LooseCancel))
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.close_loose_forms(cx);
                                    })),
                            )
                            .child(
                                Button::new("loose-create")
                                    .primary()
                                    .small()
                                    .label(tr(bardo, Text::LooseCreate))
                                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                        this.create_loose(window, cx);
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_move_form(&self, form: &MoveForm, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let differences: Vec<AnyElement> = form
            .picked
            .iter()
            .flat_map(|(_, differences)| differences.iter())
            .map(|difference| {
                let text = match *difference {
                    SettingDifference::Language { video, channel } => bardo.text_with(
                        Text::LooseDiffersLanguage,
                        &[
                            ("video", &bardo.text(Text::ContentLanguageName(video))),
                            ("channel", &bardo.text(Text::ContentLanguageName(channel))),
                        ],
                    ),
                    SettingDifference::Country { video, channel } => bardo.text_with(
                        Text::LooseDiffersCountry,
                        &[
                            ("video", &bardo.text(Text::CountryName(video))),
                            ("channel", &bardo.text(Text::CountryName(channel))),
                        ],
                    ),
                };
                kit::notice(Tone::Warning, text, cx).into_any_element()
            })
            .collect();
        let error = form
            .error
            .map(|error| kit::notice(Tone::Danger, tr(bardo, error), cx).into_any_element());
        let pick: AnyElement = if form.none {
            muted_line(cx, tr(bardo, Text::LooseMoveNoChannels)).into_any_element()
        } else {
            kit::field(
                tr(bardo, Text::LooseMoveChannel),
                None,
                Select::new(&form.channel)
                    .small()
                    .placeholder(tr(bardo, Text::LooseMoveChannel))
                    .into_any_element(),
                None,
            )
            .into_any_element()
        };
        kit::card(cx)
            .p_4()
            .child(
                v_flex()
                    .gap_3()
                    .child(kit::section_heading(tr(bardo, Text::LooseMoveTitle)))
                    .child(muted_line(cx, tr(bardo, Text::LooseMoveHint)))
                    .child(pick)
                    .children(differences)
                    .children(error)
                    .child(
                        h_flex()
                            .gap_2()
                            .justify_end()
                            .child(
                                Button::new("loose-move-cancel")
                                    .ghost()
                                    .small()
                                    .label(tr(bardo, Text::LooseCancel))
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.close_loose_forms(cx);
                                    })),
                            )
                            .child(
                                Button::new("loose-move-confirm")
                                    .primary()
                                    .small()
                                    .label(tr(bardo, Text::LooseMoveConfirm))
                                    .disabled(form.picked.is_none())
                                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                        this.confirm_move(window, cx);
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }
}

fn muted_line(cx: &gpui_kit::App, text: SharedString) -> gpui_kit::Div {
    div()
        .text_xs()
        .text_color(crate::appearance::look(cx).tokens.text2)
        .child(text)
}
