//! The missed-posts list (#82): scheduled posts whose due time passed
//! without them, shown over the window when Bardo opens, and again when one
//! is missed while it is open. Each is sent now, given a new time or
//! cancelled; "Decide later" closes the list, and each post keeps waiting on
//! its project's Publish stage. Nothing about a missed post goes without the
//! user's choice.

use std::collections::HashSet;

use bardo_app::bardo_domain::{JobKind, JobState, PublicationId};
use bardo_app::{Bardo, Control, MissedPost, MissedPostError, Side, Text, TourAnchor};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, ClickEvent, Entity, EventEmitter, SharedString, Subscription, Window, div, px,
};

use crate::appearance::look;
use crate::guide;
use crate::kit::{self, Tone};
use crate::shell::tr;
use crate::tour::Anchored as _;

/// A missed post was sent, rescheduled or cancelled: what screens show of
/// it changed.
pub struct MissedChanged;

/// What the user is doing to one missed post.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Edit {
    /// Typing its new time.
    NewTime(PublicationId),
    /// Asked to cancel it; the list asks first.
    Cancel(PublicationId),
}

pub struct MissedPosts {
    bardo: Entity<Bardo>,
    posts: Vec<MissedPost>,
    /// Put off with "Decide later" in this session.
    later: HashSet<PublicationId>,
    /// How many upload jobs had failed when the list was read: a post is
    /// missed while Bardo is open by its job failing.
    failed_uploads: usize,
    edit: Option<Edit>,
    date: Entity<InputState>,
    time: Entity<InputState>,
    /// Why the typed time cannot be used.
    problem: Option<Text>,
    /// How the last action ended.
    notice: Option<(Tone, Text)>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<MissedChanged> for MissedPosts {}

fn failed_uploads(bardo: &Bardo) -> usize {
    bardo
        .jobs()
        .iter()
        .filter(|job| job.kind() == JobKind::Upload && job.state() == JobState::Failed)
        .count()
}

impl MissedPosts {
    pub fn new(bardo: Entity<Bardo>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let date = cx.new(|cx| InputState::new(window, cx));
        let time = cx.new(|cx| InputState::new(window, cx));
        let typed = |this: &mut Self, event: &InputEvent, cx: &mut Context<Self>| {
            if matches!(event, InputEvent::Change) && this.problem.is_some() {
                this.problem = None;
                cx.notify();
            }
        };
        let subscriptions = vec![
            cx.subscribe(&date, move |this, _, event, cx| typed(this, event, cx)),
            cx.subscribe(&time, move |this, _, event, cx| typed(this, event, cx)),
        ];
        let mut list = Self {
            bardo,
            posts: Vec::new(),
            later: HashSet::new(),
            failed_uploads: 0,
            edit: None,
            date,
            time,
            problem: None,
            notice: None,
            _subscriptions: subscriptions,
        };
        list.reload(cx);
        list
    }

    /// Reads the list again; a post missed since shows it again.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let bardo = self.bardo.read(cx);
        self.failed_uploads = failed_uploads(bardo);
        self.posts = bardo.missed_posts().unwrap_or_else(|error| {
            tracing::warn!("could not read the missed posts: {error}");
            Vec::new()
        });
        let edited = self.edit.map(|edit| match edit {
            Edit::NewTime(id) | Edit::Cancel(id) => id,
        });
        if edited.is_some_and(|id| !self.posts.iter().any(|post| post.publication == id)) {
            self.edit = None;
        }
        cx.notify();
    }

    /// Reads the list again when an upload job failed: its post may have
    /// been missed.
    pub fn jobs_moved(&mut self, cx: &mut Context<Self>) {
        if failed_uploads(self.bardo.read(cx)) != self.failed_uploads {
            self.reload(cx);
        }
    }

    /// The posts the list shows: missed and not put off.
    fn shown(&self) -> impl Iterator<Item = &MissedPost> {
        self.posts
            .iter()
            .filter(|post| !self.later.contains(&post.publication))
    }

    /// Whether the list is up over the window.
    pub fn is_open(&self) -> bool {
        self.shown().next().is_some()
    }

    /// Shows the posts put off with "Decide later" again: the list's tour
    /// runs over it.
    pub fn reopen(&mut self, cx: &mut Context<Self>) {
        if !self.later.is_empty() {
            self.later.clear();
            cx.notify();
        }
    }

    /// Puts every post off for this session; the list closes.
    pub fn decide_later(&mut self, cx: &mut Context<Self>) {
        self.later
            .extend(self.posts.iter().map(|post| post.publication));
        self.edit = None;
        self.notice = None;
        cx.notify();
    }

    fn open_edit(&mut self, edit: Edit, window: &mut Window, cx: &mut Context<Self>) {
        if let Edit::NewTime(_) = edit {
            let bardo = self.bardo.read(cx);
            let (date, time) = bardo.publish_time_fields(bardo.default_publish_time());
            let date_placeholder = tr(bardo, Text::ScheduleDatePlaceholder);
            let time_placeholder = tr(bardo, Text::ScheduleTimePlaceholder);
            self.date.update(cx, |input, cx| {
                input.set_placeholder(date_placeholder, window, cx);
                input.set_value(date, window, cx);
            });
            self.time.update(cx, |input, cx| {
                input.set_placeholder(time_placeholder, window, cx);
                input.set_value(time, window, cx);
            });
        }
        self.edit = Some(edit);
        self.problem = None;
        self.notice = None;
        cx.notify();
    }

    fn send_now(&mut self, id: PublicationId, cx: &mut Context<Self>) {
        let result = self.bardo.read(cx).send_missed_now(id);
        self.finish(result, Text::MissedSent, cx);
    }

    fn reschedule(&mut self, id: PublicationId, cx: &mut Context<Self>) {
        let date = self.date.read(cx).value();
        let time = self.time.read(cx).value();
        let bardo = self.bardo.read(cx);
        let result = match bardo.publish_time(&date, &time) {
            Ok(at) => bardo.reschedule_missed(id, at),
            Err(problem) => Err(MissedPostError::Schedule(problem)),
        };
        self.finish(result, Text::MissedRescheduled, cx);
    }

    fn cancel(&mut self, id: PublicationId, cx: &mut Context<Self>) {
        let result = self.bardo.read(cx).cancel_missed(id);
        self.finish(result, Text::MissedCancelled, cx);
    }

    fn finish(&mut self, result: Result<(), MissedPostError>, done: Text, cx: &mut Context<Self>) {
        match result {
            Ok(()) => {
                self.edit = None;
                self.notice = Some((Tone::Success, done));
            }
            Err(MissedPostError::Schedule(problem)) => {
                self.problem = Some(Text::ScheduleProblem(problem));
                return cx.notify();
            }
            Err(error) => {
                self.edit = None;
                self.notice = Some((Tone::Danger, error.message()));
            }
        }
        self.reload(cx);
        cx.emit(MissedChanged);
    }

    fn post_row(&self, index: usize, post: &MissedPost, cx: &Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let id = post.publication;
        let network = bardo.text(Text::NetworkName(post.network)).into_owned();
        let handle = format!("@{}", post.handle.trim_start_matches('@'));
        let due = bardo.text_with(
            Text::MissedDue,
            &[
                ("network", &network),
                ("handle", &handle),
                ("when", &bardo.publish_time_text(post.due)),
            ],
        );
        let title = if post.title.is_empty() {
            network.clone()
        } else {
            post.title.clone()
        };
        let mut row = v_flex()
            .gap_2()
            .py_3()
            .when(index > 0, |row| {
                row.border_t(look(cx).tokens.border_width)
                    .border_color(look(cx).tokens.border)
            })
            .child(
                v_flex()
                    .gap_0p5()
                    .child(div().font_semibold().child(SharedString::from(title)))
                    .child(
                        div()
                            .text_sm()
                            .text_color(look(cx).tokens.text2)
                            .child(SharedString::from(due)),
                    ),
            );
        let button_id = |name: &str| SharedString::from(format!("missed-{name}-{index}"));
        match self.edit {
            Some(Edit::NewTime(editing)) if editing == id => {
                let field = |name: Text, input: Input| {
                    v_flex()
                        .gap_1()
                        .child(div().text_xs().font_semibold().child(tr(bardo, name)))
                        .child(div().w(px(150.)).child(input))
                };
                let zone =
                    bardo.text_with(Text::ScheduleZone, &[("zone", &bardo.time_zone_text())]);
                row = row
                    .child(
                        h_flex()
                            .gap_2()
                            .flex_wrap()
                            .child(field(Text::ScheduleDate, Input::new(&self.date).small()))
                            .child(field(Text::ScheduleTime, Input::new(&self.time).small())),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(look(cx).tokens.text2)
                            .child(SharedString::from(zone)),
                    )
                    .children(
                        self.problem
                            .map(|problem| kit::notice(Tone::Danger, tr(bardo, problem), cx)),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .justify_end()
                            .child(self.back(button_id("back"), Text::UploadBack, cx))
                            .child(
                                Button::new(button_id("save"))
                                    .small()
                                    .primary()
                                    .label(tr(bardo, Text::MissedSave))
                                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                        this.reschedule(id, cx);
                                    })),
                            ),
                    );
            }
            Some(Edit::Cancel(editing)) if editing == id => {
                row = row
                    .child(kit::notice(
                        Tone::Warning,
                        bardo.text_with(Text::MissedCancelConfirm, &[("network", &network)]),
                        cx,
                    ))
                    .child(
                        h_flex()
                            .gap_2()
                            .justify_end()
                            .child(self.back(button_id("keep"), Text::MissedKeep, cx))
                            .child(
                                Button::new(button_id("cancel-yes"))
                                    .small()
                                    .danger()
                                    .label(tr(bardo, Text::MissedCancelYes))
                                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                        this.cancel(id, cx);
                                    })),
                            ),
                    );
            }
            _ => {
                // The tour points at the first post's buttons.
                let tagged = |anchor: TourAnchor, button: Button| {
                    if index == 0 {
                        kit::anchor(anchor, button).into_any_element()
                    } else {
                        button.into_any_element()
                    }
                };
                row = row.child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .child(tagged(
                            TourAnchor::Control(Control::MissedSend),
                            Button::new(button_id("send"))
                                .small()
                                .primary()
                                .label(tr(bardo, Text::MissedSendNow))
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.send_now(id, cx);
                                })),
                        ))
                        .child(tagged(
                            TourAnchor::Control(Control::MissedNewTime),
                            Button::new(button_id("new-time"))
                                .small()
                                .outline()
                                .label(tr(bardo, Text::MissedNewTime))
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    this.open_edit(Edit::NewTime(id), window, cx);
                                })),
                        ))
                        .child(tagged(
                            TourAnchor::Control(Control::MissedCancel),
                            Button::new(button_id("cancel"))
                                .small()
                                .outline()
                                .label(tr(bardo, Text::MissedCancel))
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    this.open_edit(Edit::Cancel(id), window, cx);
                                })),
                        )),
                );
            }
        }
        row.into_any_element()
    }

    /// Closes the open edit.
    fn back(&self, id: SharedString, text: Text, cx: &Context<Self>) -> Button {
        Button::new(id)
            .small()
            .ghost()
            .label(tr(self.bardo.read(cx), text))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.edit = None;
                this.problem = None;
                cx.notify();
            }))
    }
}

impl Render for MissedPosts {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.is_open() {
            return div().into_any_element();
        }
        let shown: Vec<MissedPost> = self.shown().cloned().collect();
        let rows: Vec<AnyElement> = shown
            .iter()
            .enumerate()
            .map(|(index, post)| self.post_row(index, post, cx))
            .collect();
        let bardo = self.bardo.read(cx);
        let tokens = &look(cx).tokens;
        let card = kit::card(cx)
            .relative()
            .w(px(560.))
            .max_w_full()
            .max_h_full()
            .p_4()
            .gap_3()
            .border_color(tokens.accent_edge)
            .shadow_lg()
            .tour_anchor(TourAnchor::Control(Control::MissedList), Side::Right, None)
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .justify_between()
                    .child(kit::title(tr(bardo, Text::MissedTitle)))
                    .children(guide::missed_posts_tour(bardo, cx)),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(tokens.text2)
                    .child(tr(bardo, Text::MissedHint)),
            )
            .children(
                self.notice
                    .map(|(tone, text)| kit::notice(tone, tr(bardo, text), cx)),
            )
            .child(
                v_flex()
                    .id("missed-posts")
                    .flex_shrink_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(rows),
            )
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_xs()
                            .text_color(tokens.text2)
                            .child(tr(bardo, Text::MissedLaterHint)),
                    )
                    .child(kit::anchor(
                        TourAnchor::Control(Control::MissedLater),
                        Button::new("missed-later")
                            .small()
                            .outline()
                            .label(tr(bardo, Text::MissedLater))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.decide_later(cx);
                            })),
                    )),
            );
        // Over the whole window, which takes no clicks while it is up.
        div()
            .id("missed-overlay")
            .absolute()
            .inset_0()
            .occlude()
            .p_6()
            .flex()
            .items_center()
            .justify_center()
            .bg(tokens.app.opacity(0.8))
            .child(card)
            .into_any_element()
    }
}
