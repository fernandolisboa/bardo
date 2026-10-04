//! Jobs panel: what runs, waits, failed or finished, with cancel and retry,
//! plus buttons that start the built-in test job. The queue runs in
//! `bardo_app`; this view polls its revision and re-reads jobs when it moves,
//! so the UI thread never waits on a job.

use std::time::Duration;

use bardo_app::bardo_domain::{Job, JobId, JobState, Progress as JobProgress};
use bardo_app::{Bardo, Control, JobGroups, Side, TestJob, Text, TourAnchor};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::{
    ActiveTheme as _, IconName, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, ClickEvent, Entity, ScrollHandle, SharedString, Subscription, Task, Window, div, px,
};

use crate::appearance::look;
use crate::guide;
use crate::kit::{self, Tone};
use crate::shell::tr;
use crate::tour::Anchored as _;

/// How often the panel checks the queue for changes. Reading the revision
/// is one atomic load, so this costs nothing while jobs are idle.
const POLL_EVERY: Duration = Duration::from_millis(100);

pub struct JobsPanel {
    bardo: Entity<Bardo>,
    groups: JobGroups,
    revision: u64,
    error: Option<Text>,
    /// The list's scroll, so a tour brings the job it lights into view.
    scroll: ScrollHandle,
    _poll: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl JobsPanel {
    pub fn new(bardo: Entity<Bardo>, cx: &mut Context<Self>) -> Self {
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL_EVERY).await;
                if this.update(cx, |this, cx| this.refresh(cx)).is_err() {
                    break;
                }
            }
        });
        // Labels follow the interface language.
        let subscriptions = vec![cx.observe(&bardo, |_, _, cx| cx.notify())];
        let (groups, revision) = {
            let bardo = bardo.read(cx);
            (bardo.job_groups(), bardo.jobs_revision())
        };
        Self {
            bardo,
            groups,
            revision,
            error: None,
            scroll: ScrollHandle::new(),
            _poll: poll,
            _subscriptions: subscriptions,
        }
    }

    /// Jobs still running or waiting, for the badge on the panel's toggle.
    pub fn active(&self) -> usize {
        self.groups.active()
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let bardo = self.bardo.read(cx);
        let revision = bardo.jobs_revision();
        if revision != self.revision {
            self.groups = bardo.job_groups();
            self.revision = revision;
            cx.notify();
        }
    }

    fn start_test_job(&mut self, job: TestJob, cx: &mut Context<Self>) {
        let result = self.bardo.read(cx).start_test_job(job);
        self.error = result.err().map(|_| Text::JobNotStarted);
        self.refresh(cx);
        cx.notify();
    }

    fn cancel(&mut self, id: JobId, cx: &mut Context<Self>) {
        let result = self.bardo.read(cx).cancel_job(id);
        self.error = result.err().map(|error| error.message());
        self.refresh(cx);
        cx.notify();
    }

    fn retry(&mut self, id: JobId, cx: &mut Context<Self>) {
        let result = self.bardo.read(cx).retry_job(id);
        self.error = result.err().map(|error| error.message());
        self.refresh(cx);
        cx.notify();
    }

    fn render_job(&self, job: &Job, tagged: &mut Tagged, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let id = job.id();
        let state = job.state();
        let percent = job.progress().percent();

        let header = h_flex()
            .gap_2()
            .justify_between()
            .child(
                div()
                    .text_sm()
                    .font_medium()
                    .child(tr(bardo, Text::JobKindName(job.kind()))),
            )
            .child({
                let (tone, icon) = match state {
                    JobState::Running => (Tone::Info, IconName::Loader),
                    JobState::Queued => (Tone::Neutral, IconName::Inbox),
                    JobState::Failed => (Tone::Danger, IconName::CircleX),
                    JobState::Done => (Tone::Success, IconName::CircleCheck),
                    JobState::Cancelled => (Tone::Neutral, IconName::Ban),
                };
                kit::status_with(tone, icon, tr(bardo, Text::JobStateName(state)), cx)
            });

        // Done jobs need no bar; others show it once there is progress.
        let show_progress = state == JobState::Running
            || (state != JobState::Done && job.progress() > JobProgress::ZERO);
        let progress = show_progress.then(|| {
            let bar = h_flex()
                .gap_2()
                .child(
                    div().flex_1().child(
                        Progress::new(SharedString::from(format!("job-progress-{id}")))
                            .small()
                            .value(f32::from(percent)),
                    ),
                )
                .child(
                    div()
                        .w(px(36.))
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(SharedString::from(format!("{percent}%"))),
                );
            self.tag_first(
                &mut tagged.progress,
                TourAnchor::Control(Control::JobProgress),
                bar,
            )
        });

        let max_attempts = bardo.job_settings().retry.max_attempts;
        let status: Option<AnyElement> = match (state, job.failure()) {
            (JobState::Queued, Some(failure)) => Some(
                v_flex()
                    .gap_0p5()
                    .child(
                        kit::notice(
                            Tone::Warning,
                            bardo.text_with(
                                Text::JobRetryScheduled,
                                &[
                                    ("attempt", &job.attempts().to_string()),
                                    ("max", &max_attempts.to_string()),
                                ],
                            ),
                            cx,
                        )
                        .text_xs(),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(tr(bardo, Text::JobFailureKindName(failure.kind))),
                    )
                    .into_any_element(),
            ),
            (JobState::Failed, Some(failure)) => Some(
                h_flex()
                    .gap_1()
                    .child(
                        kit::notice(
                            Tone::Danger,
                            tr(bardo, Text::JobFailureKindName(failure.kind)),
                            cx,
                        )
                        .text_xs(),
                    )
                    .child(kit::details(
                        SharedString::from(format!("job-details-{id}")),
                        tr(bardo, Text::Details),
                        vec![
                            SharedString::from(bardo.text_with(
                                Text::JobFailedAfter,
                                &[("attempts", &job.attempts().to_string())],
                            )),
                            SharedString::from(failure.detail.clone()),
                        ],
                    ))
                    .into_any_element(),
            ),
            // A job that waits for its time (a scheduled post) says when.
            (JobState::Queued, None) => job.run_at().map(|at| {
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(SharedString::from(bardo.text_with(
                        Text::JobWaitsUntil,
                        &[("when", &bardo.publish_time_text(at))],
                    )))
                    .into_any_element()
            }),
            _ => None,
        };

        let action = if job.can_cancel() {
            let button = Button::new(SharedString::from(format!("job-cancel-{id}")))
                .xsmall()
                .outline()
                .label(tr(bardo, Text::CancelJob))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.cancel(id, cx)));
            Some(self.tag_first(
                &mut tagged.cancel,
                TourAnchor::Control(Control::JobCancel),
                button,
            ))
        } else if job.can_retry() {
            let button = Button::new(SharedString::from(format!("job-retry-{id}")))
                .xsmall()
                .outline()
                .label(tr(bardo, Text::RetryJob))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.retry(id, cx)));
            Some(self.tag_first(
                &mut tagged.retry,
                TourAnchor::Control(Control::JobRetry),
                button,
            ))
        } else {
            None
        };

        kit::card(cx)
            .p_3()
            .gap_2()
            .child(header)
            .children(progress)
            .children(status)
            .children(action.map(|button| h_flex().justify_end().child(button)))
            .into_any_element()
    }

    /// Tags `element` as `anchor` for the tour when no job before it was.
    fn tag_first(
        &self,
        done: &mut bool,
        anchor: TourAnchor,
        element: impl IntoElement,
    ) -> AnyElement {
        if std::mem::replace(done, true) {
            element.into_any_element()
        } else {
            kit::anchor_in(anchor, element, Some(&self.scroll)).into_any_element()
        }
    }

    fn render_group(
        &self,
        title: Text,
        jobs: &[Job],
        tagged: &mut Tagged,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if jobs.is_empty() {
            return None;
        }
        let heading = div()
            .text_xs()
            .font_semibold()
            .text_color(cx.theme().muted_foreground)
            .child(SharedString::from(format!(
                "{} · {}",
                tr(self.bardo.read(cx), title),
                jobs.len()
            )));
        let cards: Vec<AnyElement> = jobs
            .iter()
            .map(|job| self.render_job(job, tagged, cx))
            .collect();
        Some(
            v_flex()
                .gap_2()
                .child(heading)
                .children(cards)
                .into_any_element(),
        )
    }
}

impl Render for JobsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let groups = std::mem::take(&mut self.groups);
        let mut tagged = Tagged::default();
        let sections: Vec<AnyElement> = [
            (Text::JobsRunning, &groups.running),
            (Text::JobsQueued, &groups.queued),
            (Text::JobsFailed, &groups.failed),
            (Text::JobsFinished, &groups.finished),
        ]
        .into_iter()
        .filter_map(|(title, jobs)| self.render_group(title, jobs, &mut tagged, cx))
        .collect();
        let empty = groups.is_empty();
        let has_jobs = !empty;
        self.groups = groups;

        let bardo = self.bardo.read(cx);
        let theme = cx.theme();

        let test_jobs = kit::well(cx)
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .font_medium()
                            .child(tr(bardo, Text::TestJobsTitle)),
                    )
                    .child(guide::info(
                        bardo,
                        "test-jobs-info",
                        tr(bardo, Text::TestJobsHint),
                        guide::refs::JOBS_TEST,
                    )),
            )
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(
                        Button::new("start-test-job")
                            .small()
                            .primary()
                            .label(tr(bardo, Text::StartTestJob))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.start_test_job(TestJob::succeeding(), cx)
                            })),
                    )
                    .child(
                        Button::new("start-failing-test-job")
                            .small()
                            .outline()
                            .label(tr(bardo, Text::StartFailingTestJob))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.start_test_job(TestJob::failing(), cx)
                            })),
                    ),
            );

        let error = self
            .error
            .map(|error| kit::notice(Tone::Danger, tr(bardo, error), cx));
        let empty = empty.then(|| {
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(tr(bardo, Text::JobsEmpty))
        });

        // The panel sits at the window's right edge: its card goes left.
        v_flex()
            .relative()
            .w(px(360.))
            .h_full()
            .bg(look(cx).tokens.surface)
            .border_l_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .p_3()
                    .gap_2()
                    .justify_between()
                    .child(div().font_semibold().child(tr(bardo, Text::JobsTitle)))
                    .children(guide::panel_tour(bardo, has_jobs, cx)),
            )
            .child(
                v_flex()
                    .id("jobs-panel")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .px_3()
                    .pb_3()
                    .gap_4()
                    .child(test_jobs)
                    .children(error)
                    .children(empty)
                    .children(sections),
            )
            .tour_anchor(TourAnchor::Control(Control::JobsPanel), Side::Left, None)
    }
}

/// Which controls a tour lights are tagged already: each on the first job
/// that has one.
#[derive(Default)]
struct Tagged {
    progress: bool,
    cancel: bool,
    retry: bool,
}
