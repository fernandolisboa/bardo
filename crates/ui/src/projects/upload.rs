//! Upload with review at the Publish stage (#77). For a network Bardo
//! uploads to, the inspector offers "Review upload": a card with the file,
//! the connected channel, the title, description and tags as the network
//! gets them, the visibility, made for kids and the synthetic-content
//! disclosure. Nothing is sent until the user confirms the card; a change
//! after they opened it sends them back to a new review. The upload then
//! runs as a job the stage follows: stop, resume and retry, with the state
//! the post section shows.
//!
//! The review can also schedule the video (#78): the date and time are typed
//! in the interface language's order and the system's time zone, which the
//! card names. A scheduled upload offers "Change time" and "Cancel schedule"
//! until the network publishes it.

use std::time::SystemTime;

use bardo_app::bardo_domain::{JobId, PublicationId, ScheduleProblem, Visibility};
use bardo_app::{
    ScheduleError, ScheduleResult, Text, UploadChoices, UploadReview, UploadReviewError,
    UploadState,
};
use gpui_kit::component::button::{Button, ButtonGroup, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{
    Disableable as _, Selectable as _, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, ClickEvent, SharedString, Window, div, px};

use super::{ProjectsScreen, clock, muted};
use crate::appearance::look;
use crate::kit::{self, Tone};
use crate::shell::tr;

fn label(text: SharedString) -> gpui_kit::Div {
    div().text_xs().font_semibold().child(text)
}

/// What the user is doing to a scheduled upload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::projects) enum ScheduleEdit {
    /// Typing a new publish time.
    Change,
    /// Asked to cancel the schedule; the stage asks first.
    Cancel,
}

impl ProjectsScreen {
    /// Reads the shown network's upload review, when Bardo uploads to it.
    /// An open review of another network closes.
    pub(super) fn load_upload(&mut self, cx: &mut Context<Self>) {
        let bardo = self.bardo.read(cx);
        let shown = self
            .export_view
            .as_ref()
            .and_then(|view| self.shown_network(view))
            .map(|target| target.network);
        self.upload_now = match (self.project, shown) {
            (Some(project), Some(network)) if bardo.uploads_to(network) => {
                bardo.upload_review(project, network).ok()
            }
            _ => None,
        };
        if self
            .upload_draft
            .as_ref()
            .is_some_and(|(review, _)| Some(review.network) != shown)
        {
            self.upload_draft = None;
        }
        // A change of time only stays open while the upload is scheduled.
        if self.scheduled_upload(cx).is_none() {
            self.schedule_edit = None;
        }
    }

    /// The shown network's upload when it is scheduled and its job is done:
    /// its publication and publish time.
    fn scheduled_upload(&self, cx: &Context<Self>) -> Option<(PublicationId, SystemTime)> {
        let publication = self.upload_now.as_ref()?.replaces.as_ref()?;
        match self.bardo.read(cx).upload_state(publication)? {
            UploadState::Scheduled(at) => Some((publication.id, at)),
            _ => None,
        }
    }

    fn open_upload_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(review) = self.upload_now.clone() {
            let choices = review.choices();
            self.upload_draft = Some((review, choices));
            self.upload_error = None;
            self.upload_scheduled = false;
            self.schedule_edit = None;
            self.schedule_notice = None;
            let at = self.bardo.read(cx).default_publish_time();
            self.fill_schedule(at, window, cx);
        }
        cx.notify();
    }

    /// Places `at` in the date and time fields.
    fn fill_schedule(&mut self, at: SystemTime, window: &mut Window, cx: &mut Context<Self>) {
        let (date, time) = self.bardo.read(cx).publish_time_fields(at);
        self.schedule_date
            .update(cx, |input, cx| input.set_value(date, window, cx));
        self.schedule_time
            .update(cx, |input, cx| input.set_value(time, window, cx));
        self.schedule_problem = None;
    }

    /// Typing a publish time retires the problem shown for the last one.
    pub(super) fn schedule_typed(&mut self, event: &InputEvent, cx: &mut Context<Self>) {
        if matches!(event, InputEvent::Change) && self.schedule_problem.is_some() {
            self.schedule_problem = None;
            cx.notify();
        }
    }

    /// The publish time the fields hold.
    fn typed_publish_time(&self, cx: &Context<Self>) -> Result<SystemTime, ScheduleProblem> {
        let date = self.schedule_date.read(cx).value();
        let time = self.schedule_time.read(cx).value();
        self.bardo.read(cx).publish_time(&date, &time)
    }

    fn open_schedule_edit(
        &mut self,
        edit: ScheduleEdit,
        at: SystemTime,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if edit == ScheduleEdit::Change {
            self.fill_schedule(at, window, cx);
        }
        self.schedule_edit = Some(edit);
        self.schedule_notice = None;
        self.upload_error = None;
        cx.notify();
    }

    /// Sends the new publish time, or the cancellation (`cancel`), to the
    /// network on a background thread; the stage shows how it ended.
    fn send_schedule_change(
        &mut self,
        publication: PublicationId,
        cancel: bool,
        cx: &mut Context<Self>,
    ) {
        let publish_at = if cancel {
            None
        } else {
            match self.typed_publish_time(cx) {
                Ok(at) => Some(at),
                Err(problem) => {
                    self.schedule_problem = Some(Text::ScheduleProblem(problem));
                    cx.notify();
                    return;
                }
            }
        };
        let update = match self.bardo.read(cx).schedule_change(publication, publish_at) {
            Ok(update) => update,
            Err(ScheduleError::Problem(problem)) => {
                self.schedule_problem = Some(Text::ScheduleProblem(problem));
                cx.notify();
                return;
            }
            Err(error) => {
                self.schedule_edit = None;
                self.schedule_notice = Some((Tone::Danger, error.message()));
                cx.notify();
                return;
            }
        };
        let bardo = self.bardo.clone();
        self.schedule_task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { update.run() })
                .await;
            bardo.update(cx, |_, cx| cx.notify());
            let _ = this.update_in(cx, |this, window, cx| {
                this.schedule_task = None;
                this.schedule_notice = Some(match result {
                    Ok(ScheduleResult::Rescheduled(_)) => {
                        this.schedule_edit = None;
                        (Tone::Success, Text::ScheduleChanged)
                    }
                    Ok(ScheduleResult::Cancelled) => {
                        this.schedule_edit = None;
                        (Tone::Success, Text::ScheduleCancelled)
                    }
                    Ok(ScheduleResult::AlreadyLive) => {
                        this.schedule_edit = None;
                        (Tone::Warning, Text::ScheduleAlreadyLive)
                    }
                    Err(ScheduleError::Problem(problem)) => {
                        this.schedule_problem = Some(Text::ScheduleProblem(problem));
                        (Tone::Danger, Text::ScheduleProblem(problem))
                    }
                    Err(error) => (Tone::Danger, error.message()),
                });
                if this.schedule_problem.is_some() {
                    this.schedule_notice = None;
                }
                this.load(window, cx);
                cx.notify();
            });
        }));
        self.schedule_notice = None;
        cx.notify();
    }

    fn change_upload(&mut self, change: impl FnOnce(&mut UploadChoices), cx: &mut Context<Self>) {
        if let Some((_, choices)) = self.upload_draft.as_mut() {
            change(choices);
        }
        cx.notify();
    }

    /// Starts the upload the user reviewed. A review that no longer holds
    /// closes, so the next one shows what changed.
    fn confirm_upload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((review, mut choices)) = self.upload_draft.clone() else {
            return;
        };
        if self.upload_scheduled {
            match self.typed_publish_time(cx) {
                Ok(at) => choices.publish_at = Some(at),
                Err(problem) => {
                    self.schedule_problem = Some(Text::ScheduleProblem(problem));
                    cx.notify();
                    return;
                }
            }
        }
        match self.bardo.read(cx).start_upload(&review, choices) {
            Ok(_) => {
                self.upload_draft = None;
                self.upload_error = None;
                self.export_notice = Some(Text::UploadQueued);
            }
            Err(UploadReviewError::Schedule(problem)) => {
                self.schedule_problem = Some(Text::ScheduleProblem(problem));
            }
            Err(error) => {
                if matches!(
                    error,
                    UploadReviewError::Changed | UploadReviewError::Blocked(_)
                ) {
                    self.upload_draft = None;
                }
                self.upload_error = Some(error.message());
            }
        }
        self.load(window, cx);
        cx.notify();
    }

    fn upload_job_action(
        &mut self,
        job: JobId,
        resume: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let bardo = self.bardo.read(cx);
        self.upload_error = if resume {
            bardo.resume_upload(job).err().map(|error| error.message())
        } else {
            bardo.cancel_job(job).err().map(|_| Text::UploadNotStarted)
        };
        self.load(window, cx);
        cx.notify();
    }

    fn check_upload_again(
        &mut self,
        publication: PublicationId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let bardo = self.bardo.read(cx);
        self.upload_error = bardo
            .check_upload(publication)
            .err()
            .map(|error| error.message());
        self.load(window, cx);
        cx.notify();
    }

    /// The shown network's upload: the open review, or what the upload
    /// does now and the way to review one. `None` for a network Bardo
    /// does not upload to.
    pub(super) fn upload_section(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let now = self.upload_now.as_ref()?;
        let bardo = self.bardo.read(cx);
        let heading = h_flex()
            .gap_1()
            .items_center()
            .child(label(tr(bardo, Text::UploadTitle)))
            .child(kit::info("upload-info", None, tr(bardo, Text::UploadHint)));
        let mut section = v_flex().gap_2().child(heading);
        if let Some((review, choices)) = &self.upload_draft {
            return Some(
                section
                    .child(self.upload_card(review, *choices, cx))
                    .into_any_element(),
            );
        }

        // The current upload's job: stop it, or resume or retry it.
        let upload = now.replaces.as_ref().and_then(|publication| {
            Some((publication.upload()?.job, bardo.upload_state(publication)?))
        });
        let shown = now.replaces.as_ref().map(|publication| publication.id);
        let active = upload.as_ref().is_some_and(|(_, state)| state.is_active());
        let mut actions = h_flex().gap_2().items_center().flex_wrap();
        match upload {
            Some((job, _)) if active => {
                actions = actions.child(
                    Button::new("upload-stop")
                        .small()
                        .outline()
                        .label(tr(bardo, Text::UploadStop))
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.upload_job_action(job, false, window, cx);
                        })),
                );
            }
            Some((job, UploadState::Stopped)) => {
                actions = actions.child(
                    Button::new("upload-resume")
                        .small()
                        .primary()
                        .label(tr(bardo, Text::UploadResume))
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.upload_job_action(job, true, window, cx);
                        })),
                );
            }
            Some((
                job,
                UploadState::Failed {
                    retryable: true, ..
                },
            )) => {
                actions = actions.child(
                    Button::new("upload-retry")
                        .small()
                        .outline()
                        .label(tr(bardo, Text::UploadRetry))
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.upload_job_action(job, true, window, cx);
                        })),
                );
            }
            Some((_, UploadState::StillProcessing)) => {
                actions = actions.child(
                    Button::new("upload-check")
                        .small()
                        .outline()
                        .label(tr(bardo, Text::UploadCheckAgain))
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            if let Some(publication) = shown {
                                this.check_upload_again(publication, window, cx);
                            }
                        })),
                );
            }
            _ => {}
        }
        // A scheduled upload: change its time or cancel the schedule.
        let scheduled = self.scheduled_upload(cx).filter(|_| !active);
        let idle = self.schedule_edit.is_none() && self.schedule_task.is_none();
        if let Some((_, at)) = scheduled.filter(|_| idle) {
            actions = actions
                .child(
                    Button::new("schedule-change")
                        .small()
                        .outline()
                        .label(tr(bardo, Text::ScheduleChange))
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.open_schedule_edit(ScheduleEdit::Change, at, window, cx);
                        })),
                )
                .child(
                    Button::new("schedule-cancel")
                        .small()
                        .outline()
                        .label(tr(bardo, Text::ScheduleCancel))
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.open_schedule_edit(ScheduleEdit::Cancel, at, window, cx);
                        })),
                );
        }
        let block = now.block();
        if !active {
            actions = actions.child(
                Button::new("upload-review")
                    .small()
                    .when(block.is_none(), |button| button.primary())
                    .when(block.is_some(), |button| button.outline())
                    .label(tr(bardo, Text::UploadOpenReview))
                    .disabled(block.is_some())
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.open_upload_review(window, cx);
                    })),
            );
        }
        section = section.child(actions);
        if let Some(block) = block.filter(|_| !active) {
            section = section.child(muted(cx, tr(bardo, Text::UploadBlocked(block))));
        }
        if let Some((publication, _)) = scheduled {
            section = section.children(self.schedule_editor(publication, cx));
        }
        let network = bardo.text(Text::NetworkName(now.network)).into_owned();
        let with_network = |text: Text| bardo.text_with(text, &[("network", &network)]);
        if self.schedule_task.is_some() {
            section = section.child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(Spinner::new().small())
                    .child(
                        div()
                            .text_sm()
                            .child(SharedString::from(with_network(Text::ScheduleWorking))),
                    ),
            );
        }
        section = section.children(
            self.schedule_notice
                .map(|(tone, text)| kit::notice(tone, with_network(text), cx)),
        );
        section = section.children(
            self.upload_error
                .map(|error| kit::notice(Tone::Danger, tr(bardo, error), cx)),
        );
        Some(section.into_any_element())
    }

    /// The date and time fields with the zone they are read in, and why the
    /// typed time cannot be used.
    fn schedule_fields(&self, cx: &Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let field = |name: Text, input: Input| {
            v_flex()
                .gap_1()
                .child(label(tr(bardo, name)))
                .child(div().w(px(150.)).child(input))
        };
        let zone = bardo.text_with(Text::ScheduleZone, &[("zone", &bardo.time_zone_text())]);
        v_flex()
            .gap_1p5()
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(field(
                        Text::ScheduleDate,
                        Input::new(&self.schedule_date).small(),
                    ))
                    .child(field(
                        Text::ScheduleTime,
                        Input::new(&self.schedule_time).small(),
                    )),
            )
            .child(muted(cx, SharedString::from(zone)))
            .children(
                self.schedule_problem
                    .map(|problem| kit::notice(Tone::Danger, tr(bardo, problem), cx)),
            )
            .into_any_element()
    }

    /// The open change of a scheduled upload: the new time, or the question
    /// before cancelling. Nothing while the change is sent.
    fn schedule_editor(
        &self,
        publication: PublicationId,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        let edit = self
            .schedule_edit
            .filter(|_| self.schedule_task.is_none())?;
        let bardo = self.bardo.read(cx);
        let network = self
            .upload_now
            .as_ref()
            .map(|now| bardo.text(Text::NetworkName(now.network)).into_owned())
            .unwrap_or_default();
        let back = |id: &'static str, text: Text| {
            Button::new(id)
                .small()
                .ghost()
                .label(tr(bardo, text))
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.schedule_edit = None;
                    this.schedule_problem = None;
                    cx.notify();
                }))
        };
        let editor = match edit {
            ScheduleEdit::Change => v_flex().gap_2().child(self.schedule_fields(cx)).child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(back("schedule-back", Text::UploadBack))
                    .child(
                        Button::new("schedule-save")
                            .small()
                            .primary()
                            .label(tr(bardo, Text::ScheduleSave))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.send_schedule_change(publication, false, cx);
                            })),
                    ),
            ),
            ScheduleEdit::Cancel => v_flex()
                .gap_2()
                .child(kit::notice(
                    Tone::Warning,
                    bardo.text_with(Text::ScheduleCancelConfirm, &[("network", &network)]),
                    cx,
                ))
                .child(
                    h_flex()
                        .gap_2()
                        .justify_end()
                        .child(back("schedule-keep", Text::ScheduleKeep))
                        .child(
                            Button::new("schedule-cancel-yes")
                                .small()
                                .danger()
                                .label(tr(bardo, Text::ScheduleCancelYes))
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.send_schedule_change(publication, true, cx);
                                })),
                        ),
                ),
        };
        Some(
            kit::card(cx)
                .p_3()
                .gap_2()
                .border_color(look(cx).tokens.accent_edge)
                .child(editor)
                .into_any_element(),
        )
    }

    /// The review: what goes, where, and the choices the user makes.
    fn upload_card(
        &self,
        review: &UploadReview,
        choices: UploadChoices,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let network = bardo.text(Text::NetworkName(review.network)).into_owned();
        let with_network = |text: Text| bardo.text_with(text, &[("network", &network)]);
        let row = |name: Text, value: AnyElement| {
            v_flex()
                .gap_1()
                .child(label(tr(bardo, name)))
                .child(value)
                .into_any_element()
        };
        let text = |value: String| {
            div()
                .text_sm()
                .child(SharedString::from(value))
                .into_any_element()
        };

        let mut card = kit::card(cx)
            .p_3()
            .gap_3()
            .border_color(look(cx).tokens.accent_edge)
            .child(
                div()
                    .font_semibold()
                    .child(SharedString::from(with_network(Text::UploadReviewTitle))),
            );
        if let Some(render) = &review.render {
            let (width, height) = render.preset.dimensions();
            card = card.child(row(
                Text::UploadFieldFile,
                text(format!(
                    "{} · {width}×{height} · {} · {}",
                    render.file,
                    clock(render.duration),
                    bardo.render_file_line(render)
                )),
            ));
        }
        card = card.child(row(
            Text::UploadFieldChannel,
            text(format!(
                "{} · @{}",
                review.channel().unwrap_or_default(),
                review.handle
            )),
        ));
        if let Some(post) = &review.post {
            if let Some(title) = &post.title {
                card = card.child(row(Text::MetadataFieldTitle, text(title.clone())));
            }
            if let Some(description) = &post.text {
                card = card.child(row(
                    Text::MetadataFieldDescription,
                    kit::well(cx)
                        .text_xs()
                        .child(SharedString::from(description.clone()))
                        .into_any_element(),
                ));
            }
            card = card.child(row(Text::MetadataFieldTags, text(post.tags.join(", "))));
        }

        let visibility = ButtonGroup::new("upload-visibility")
            .outline()
            .small()
            .children(Visibility::ALL.map(|visibility| {
                Button::new(visibility.code())
                    .label(tr(bardo, Text::VisibilityName(visibility)))
                    .selected(visibility == choices.visibility)
            }))
            .on_click(cx.listener(|this, clicked: &Vec<usize>, _, cx| {
                if let Some(visibility) = clicked.first().and_then(|&i| Visibility::ALL.get(i)) {
                    let visibility = *visibility;
                    this.change_upload(|choices| choices.visibility = visibility, cx);
                }
            }));
        let scheduled = self.upload_scheduled;
        let when = ButtonGroup::new("upload-when")
            .outline()
            .small()
            .child(
                Button::new("upload-when-now")
                    .label(tr(bardo, Text::UploadWhenNow))
                    .selected(!scheduled),
            )
            .child(
                Button::new("upload-when-schedule")
                    .label(tr(bardo, Text::UploadWhenSchedule))
                    .selected(scheduled),
            )
            .on_click(cx.listener(|this, clicked: &Vec<usize>, _, cx| {
                if let Some(&i) = clicked.first() {
                    this.upload_scheduled = i == 1;
                    this.schedule_problem = None;
                    cx.notify();
                }
            }));
        card = card.child(row(
            Text::UploadFieldWhen,
            h_flex().child(when).into_any_element(),
        ));
        if scheduled {
            card = card.child(self.schedule_fields(cx)).child(muted(
                cx,
                SharedString::from(with_network(Text::ScheduleHint)),
            ));
        } else {
            card = card.child(row(
                Text::UploadFieldVisibility,
                h_flex().child(visibility).into_any_element(),
            ));
        }

        let screen = cx.entity().downgrade();
        let kids = Checkbox::new("upload-kids")
            .label(tr(bardo, Text::UploadMadeForKids))
            .checked(choices.made_for_kids)
            .on_click({
                let screen = screen.clone();
                move |checked: &bool, _, cx| {
                    let checked = *checked;
                    let _ = screen.update(cx, |this, cx| {
                        this.change_upload(|choices| choices.made_for_kids = checked, cx)
                    });
                }
            });
        card = card.child(h_flex().gap_1().items_center().child(kids).child(kit::info(
            "upload-kids-info",
            None,
            tr(bardo, Text::UploadMadeForKidsHint),
        )));
        let synthetic = Checkbox::new("upload-synthetic")
            .label(tr(bardo, Text::UploadSynthetic))
            .checked(choices.synthetic)
            .on_click({
                let screen = screen.clone();
                move |checked: &bool, _, cx| {
                    let checked = *checked;
                    let _ = screen.update(cx, |this, cx| {
                        this.change_upload(|choices| choices.synthetic = checked, cx)
                    });
                }
            });
        card = card.child(
            v_flex()
                .gap_1()
                .child(
                    h_flex()
                        .gap_1()
                        .items_center()
                        .child(synthetic)
                        .child(kit::info(
                            "upload-synthetic-info",
                            None,
                            tr(bardo, Text::UploadSyntheticHint),
                        )),
                )
                .when(review.synthetic, |column| {
                    column.child(muted(cx, tr(bardo, Text::UploadSyntheticOn)))
                }),
        );

        if let Some(replaced) = &review.replaces {
            let text = if replaced.upload().is_some() {
                Text::UploadReplaceUpload
            } else {
                Text::UploadReplacePost
            };
            let replace = Checkbox::new("upload-replace")
                .label(SharedString::from(with_network(text)))
                .checked(choices.replace)
                .on_click(move |checked: &bool, _, cx| {
                    let checked = *checked;
                    let _ = screen.update(cx, |this, cx| {
                        this.change_upload(|choices| choices.replace = checked, cx)
                    });
                });
            card = card.child(replace);
        }

        card = card.child(kit::notice(
            Tone::Warning,
            with_network(Text::UploadIrreversible),
            cx,
        ));
        let ready = review.replaces.is_none() || choices.replace;
        card = card.child(
            h_flex()
                .gap_2()
                .justify_end()
                .child(
                    Button::new("upload-back")
                        .small()
                        .ghost()
                        .label(tr(bardo, Text::UploadBack))
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.upload_draft = None;
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("upload-confirm")
                        .small()
                        .primary()
                        .label(tr(
                            bardo,
                            if scheduled {
                                Text::UploadStartScheduled
                            } else {
                                Text::UploadStart
                            },
                        ))
                        .disabled(!ready)
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.confirm_upload(window, cx);
                        })),
                ),
        );
        card.into_any_element()
    }
}
