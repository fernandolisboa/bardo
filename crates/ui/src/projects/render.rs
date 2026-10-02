//! The Render stage of the projects screen (#27): the review before the
//! final render. The cut's figures (length, frame, mix loudness,
//! captions) lead; every network account of the channel is a target in
//! its render preset, with its checks, and the picked one opens in the
//! inspector with its encoder and last file. Checking the machine's
//! encoders and measuring the mix take a few seconds of ffmpeg, so they
//! run off the UI thread when the stage opens; what they found is kept
//! while the cut stays the same. Rendering is irreversible in time: the
//! Render button asks for confirmation, then queues one job that the
//! stage follows (progress, cancel, resume).

use std::rc::Rc;

use bardo_app::bardo_domain::{Gate, GateLevel, Job, JobState, NetworkAccountId, VideoProjectId};
use bardo_app::{Bardo, RenderError, RenderReview, RenderTarget, Stage, Text, render_job_files};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::progress::Progress;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, ClickEvent, SharedString, Window, div, px};

use super::{ProjectsScreen, clock, muted};
use crate::appearance::look;
use crate::kit::{self, Tone};
use crate::parts::{Collection, CollectionKind, Figure, Inspector, ScreenParts, Tile};
use crate::shell::tr;

/// How a target stands in the review.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TargetState {
    /// The machine's encoders and the mix are not known yet.
    Checking,
    Blocked,
    /// Renders, with this many warnings to look at.
    Warnings(usize),
    Ready,
}

fn target_state(review: &RenderReview, target: &RenderTarget) -> TargetState {
    if !review.is_checked() {
        TargetState::Checking
    } else if !target.can_render() {
        TargetState::Blocked
    } else if target.gates.is_empty() {
        TargetState::Ready
    } else {
        TargetState::Warnings(target.gates.len())
    }
}

fn state_chip(bardo: &Bardo, state: TargetState, cx: &App) -> AnyElement {
    match state {
        TargetState::Checking => {
            kit::status(Tone::Neutral, tr(bardo, Text::RenderStateChecking), cx)
        }
        TargetState::Blocked => kit::status(Tone::Danger, tr(bardo, Text::RenderStateBlocked), cx),
        TargetState::Warnings(n) => kit::status(
            Tone::Warning,
            bardo.text_with(Text::RenderStateWarnings, &[("n", &n.to_string())]),
            cx,
        ),
        TargetState::Ready => kit::status(Tone::Success, tr(bardo, Text::RenderStateReady), cx),
    }
    .into_any_element()
}

/// A gate as a line: its level as a chip, then what it means.
fn gate_line(bardo: &Bardo, gate: &Gate, cx: &App) -> AnyElement {
    let (tone, level) = match gate.level() {
        GateLevel::Blocking => (Tone::Danger, Text::GateBlocks),
        GateLevel::Warning => (Tone::Warning, Text::GateWarning),
    };
    h_flex()
        .gap_2()
        .items_start()
        .child(kit::status(tone, tr(bardo, level), cx))
        .child(
            div()
                .min_w_0()
                .text_sm()
                .child(SharedString::from(bardo.gate_text(gate))),
        )
        .into_any_element()
}

fn field_label(text: SharedString) -> gpui_kit::Div {
    div().text_xs().font_semibold().child(text)
}

fn is_running(job: Option<&Job>) -> bool {
    job.is_some_and(|job| job.state().is_active())
}

impl ProjectsScreen {
    /// Reads the project's render review and summary, with what the last
    /// check found when it was about this cut.
    pub(super) fn load_render(&mut self, id: VideoProjectId, cx: &mut Context<Self>) {
        let bardo = self.bardo.read(cx);
        self.render_summary = bardo.render_summary(id).ok();
        self.render_review = match bardo.render_review(id) {
            Ok(review) => Some(match &self.render_found {
                Some(found) => review.checked(found),
                None => review,
            }),
            Err(RenderError::NoCut) => None,
            Err(error) => {
                self.render_error = Some(error.message());
                None
            }
        };
        self.ensure_render_checked(cx);
    }

    /// Starts checking the review when the Render stage is on screen and
    /// nothing found so far is about this cut.
    pub(super) fn ensure_render_checked(&mut self, cx: &mut Context<Self>) {
        let unchecked = self
            .render_review
            .as_ref()
            .is_some_and(|review| !review.is_checked());
        if self.stage == Stage::Render
            && unchecked
            && self.render_checking.is_none()
            && !self.render_check_failed
        {
            self.check_render(cx);
        }
    }

    /// Tries the encoders and measures the mix off the UI thread.
    fn check_render(&mut self, cx: &mut Context<Self>) {
        let Some(review) = self.render_review.as_ref() else {
            return;
        };
        let project = review.project;
        let checks = self.bardo.read(cx).render_checks(review);
        self.render_check_failed = false;
        self.render_checking = Some(cx.spawn(async move |this, cx| {
            let found = cx
                .background_executor()
                .spawn(async move { checks.run() })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.render_checking = None;
                if this.project != Some(project) {
                    return;
                }
                match found {
                    Ok(found) => {
                        this.render_review = this
                            .render_review
                            .take()
                            .map(|review| review.checked(&found));
                        this.render_found = Some(found);
                    }
                    Err(_) => {
                        this.render_check_failed = true;
                        this.render_error = Some(Text::RenderCheckFailed);
                    }
                }
                cx.notify();
            });
        }));
    }

    fn check_render_again(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.render_found = None;
        self.render_check_failed = false;
        self.render_error = None;
        self.render_checking = None;
        self.load(window, cx);
        cx.notify();
    }

    /// Whether `target` is in the render: as the user ticked it, else
    /// when its last file is missing or out of date.
    fn is_chosen(&self, target: &RenderTarget) -> bool {
        self.render_choices
            .iter()
            .find(|(account, _)| *account == target.account)
            .map_or(!target.last_current, |(_, chosen)| *chosen)
    }

    /// The targets the render would make: the renderable ones chosen.
    fn chosen_targets(&self) -> Vec<NetworkAccountId> {
        let Some(review) = self.render_review.as_ref() else {
            return Vec::new();
        };
        let renderable = review.renderable();
        review
            .targets
            .iter()
            .filter(|target| renderable.contains(&target.account) && self.is_chosen(target))
            .map(|target| target.account)
            .collect()
    }

    fn toggle_target(&mut self, account: NetworkAccountId, include: bool, cx: &mut Context<Self>) {
        self.render_choices.retain(|(id, _)| *id != account);
        self.render_choices.push((account, include));
        self.confirm_render = false;
        cx.notify();
    }

    fn start_render(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_render = false;
        let Some(review) = self.render_review.as_ref() else {
            return;
        };
        let chosen = self.chosen_targets();
        match self.bardo.read(cx).start_render(review, &chosen) {
            // The next review chooses again from what is out of date.
            Ok(_) => {
                self.render_choices.clear();
                self.render_error = None;
            }
            Err(error) => self.render_error = Some(error.message()),
        }
        self.load(window, cx);
        cx.notify();
    }

    fn render_job_action(&mut self, resume: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(job) = self.render_summary.as_ref().and_then(|s| s.job.as_ref()) else {
            return;
        };
        let bardo = self.bardo.read(cx);
        let result = if resume {
            bardo.retry_job(job.id())
        } else {
            bardo.cancel_job(job.id())
        };
        if result.is_err() {
            self.render_error = Some(Text::RenderNotLoaded);
        }
        self.load(window, cx);
        cx.notify();
    }

    /// The target in the inspector: the picked one, else the first.
    fn shown_target<'a>(&self, review: &'a RenderReview) -> Option<&'a RenderTarget> {
        review
            .targets
            .iter()
            .find(|target| Some(target.account) == self.selected_target)
            .or(review.targets.first())
    }

    /// The Render stage: the cut's figures, the toolbar, what happened and
    /// the cut's own checks, the targets, and the picked one.
    pub(super) fn render_parts(&self, parts: &mut ScreenParts, cx: &mut Context<Self>) {
        let Some(review) = self.render_review.as_ref() else {
            let bardo = self.bardo.read(cx);
            parts.notices.extend(
                self.render_error.map(|error| {
                    kit::notice(Tone::Danger, tr(bardo, error), cx).into_any_element()
                }),
            );
            return;
        };
        let job = self.render_summary.as_ref().and_then(|s| s.job.as_ref());
        parts.summary = self.render_figures(review, cx);
        parts.toolbar = Some(self.render_toolbar(review, job, cx));
        parts.notices.extend(self.render_notices(review, job, cx));
        parts.collection = Some(self.render_targets(review, job, cx));
        parts.inspector = self
            .shown_target(review)
            .map(|target| self.target_inspector(review, target, job, cx));
    }

    fn render_figures(&self, review: &RenderReview, cx: &App) -> Vec<Figure> {
        let bardo = self.bardo.read(cx);
        let cut = &review.cut;
        let loudness = match &cut.mix {
            Some(mix) => bardo.loudness_text(mix),
            None => bardo.text(Text::RenderMeasuring).into_owned(),
        };
        let mut loudness = Figure::new(tr(bardo, Text::RenderFigureLoudness), loudness);
        if cut.mix.is_some_and(|mix| mix.is_silent()) {
            loudness.tone = Some(Tone::Warning);
        }
        let mut captions = Figure::new(
            tr(bardo, Text::RenderFigureCaptions),
            tr(
                bardo,
                if cut.captions_shown {
                    Text::RenderCaptionsOn
                } else {
                    Text::RenderCaptionsOff
                },
            ),
        );
        if !cut.captions_shown {
            captions.tone = Some(Tone::Warning);
        }
        vec![
            Figure::new(tr(bardo, Text::RenderFigureLength), clock(cut.duration)),
            Figure::new(tr(bardo, Text::RenderFigureFrame), cut.aspect.code()),
            loudness,
            captions,
        ]
    }

    /// What the stage does: how many are chosen, then render (or the
    /// running render's progress and cancel, or resume).
    fn render_toolbar(
        &self,
        review: &RenderReview,
        job: Option<&Job>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // An export copies the rendered files: rendering waits for it.
        let exporting = is_running(
            self.export_summary
                .as_ref()
                .and_then(|summary| summary.job.as_ref()),
        );
        let chosen = self.chosen_targets().len();
        let running = is_running(job);
        let stopped = job.is_some_and(|job| {
            matches!(job.state(), JobState::Failed | JobState::Cancelled) && job.can_retry()
        });
        let bardo = self.bardo.read(cx);
        let mut row = h_flex().gap_2().flex_wrap().items_center().child(kit::info(
            "render-info",
            None,
            tr(bardo, Text::RenderInfo),
        ));
        if review.is_checked() && !running {
            row = row.child(div().text_sm().text_color(look(cx).tokens.text2).child(
                SharedString::from(bardo.text_with(
                    Text::RenderChosen,
                    &[
                        ("n", &chosen.to_string()),
                        ("total", &review.targets.len().to_string()),
                    ],
                )),
            ));
        }
        if self.render_checking.is_some() {
            row = row
                .child(Spinner::new().small())
                .child(div().text_sm().child(tr(bardo, Text::RenderChecking)));
        }
        row = row.child(div().flex_1());
        if let Some(job) = job.filter(|_| running) {
            let (done, total) = render_job_files(job);
            row = row
                .child(div().text_sm().child(SharedString::from(bardo.text_with(
                    Text::RenderRunning,
                    &[
                        ("done", &(done + 1).min(total.max(1)).to_string()),
                        ("total", &total.to_string()),
                    ],
                ))))
                .child(
                    div().w(px(160.)).child(
                        Progress::new("render-progress")
                            .small()
                            .value(job.progress().permille() as f32 / 10.0),
                    ),
                )
                .child(
                    Button::new("render-cancel")
                        .small()
                        .outline()
                        .label(tr(bardo, Text::CancelJob))
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.render_job_action(false, window, cx);
                        })),
                );
            return row.into_any_element();
        }
        let checkable =
            self.render_checking.is_none() && (review.is_checked() || self.render_check_failed);
        if checkable {
            row = row.child(
                Button::new("render-check-again")
                    .small()
                    .ghost()
                    .label(tr(bardo, Text::RenderCheckAgain))
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.check_render_again(window, cx);
                    })),
            );
        }
        if stopped && !exporting {
            row = row.child(
                Button::new("render-resume")
                    .small()
                    .outline()
                    .label(tr(bardo, Text::RenderResume))
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.render_job_action(true, window, cx);
                    })),
            );
        }
        if chosen > 0 && !self.confirm_render && !exporting {
            row = row.child(
                Button::new("render-start")
                    .small()
                    .primary()
                    .label(SharedString::from(
                        bardo.text_with(Text::RenderStart, &[("n", &chosen.to_string())]),
                    ))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.confirm_render = true;
                        cx.notify();
                    })),
            );
        }
        row.into_any_element()
    }

    /// What happened (an error, a stopped render), the confirmation, and
    /// the checks about the cut itself.
    fn render_notices(
        &self,
        review: &RenderReview,
        job: Option<&Job>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut notices = Vec::new();
        if self.confirm_render {
            notices.push(self.render_confirm(review, cx));
        }
        let bardo = self.bardo.read(cx);
        if let Some(error) = self.render_error {
            notices.push(kit::notice(Tone::Danger, tr(bardo, error), cx).into_any_element());
        }
        match job.map(|job| (job.state(), job)) {
            Some((JobState::Failed, job)) => {
                let detail = job
                    .failure()
                    .map(|failure| SharedString::from(failure.detail.clone()));
                notices.push(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(kit::notice(
                            Tone::Danger,
                            tr(bardo, Text::RenderStopped),
                            cx,
                        ))
                        .children(detail.map(|detail| {
                            kit::details("render-failure", tr(bardo, Text::Details), vec![detail])
                        }))
                        .into_any_element(),
                );
            }
            Some((JobState::Cancelled, _)) => notices.push(
                kit::notice(Tone::Warning, tr(bardo, Text::RenderCancelled), cx).into_any_element(),
            ),
            _ => {}
        }
        notices.extend(review.gates.iter().map(|gate| gate_line(bardo, gate, cx)));
        if review.targets.is_empty() {
            notices.push(muted(cx, tr(bardo, Text::RenderNoAccounts)));
        }
        notices
    }

    /// Asks before the render starts: how many files, that it replaces the
    /// last ones, and the warnings left.
    fn render_confirm(&self, review: &RenderReview, cx: &mut Context<Self>) -> AnyElement {
        let chosen = self.chosen_targets();
        let warnings = review.gates.len()
            + review
                .targets
                .iter()
                .filter(|target| chosen.contains(&target.account))
                .map(|target| target.gates.len())
                .sum::<usize>();
        let bardo = self.bardo.read(cx);
        let t = look(cx).tokens;
        kit::card(cx)
            .p_4()
            .gap_2()
            .border_color(t.accent_edge)
            .child(
                div()
                    .font_semibold()
                    .child(SharedString::from(bardo.text_with(
                        Text::RenderConfirmTitle,
                        &[("n", &chosen.len().to_string())],
                    ))),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(t.text2)
                    .child(tr(bardo, Text::RenderConfirmBody)),
            )
            .when(warnings > 0, |card| {
                card.child(kit::notice(
                    Tone::Warning,
                    bardo.text_with(Text::RenderConfirmWarnings, &[("n", &warnings.to_string())]),
                    cx,
                ))
            })
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("render-confirm-back")
                            .small()
                            .ghost()
                            .label(tr(bardo, Text::RenderConfirmBack))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.confirm_render = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("render-confirm")
                            .small()
                            .primary()
                            .label(tr(bardo, Text::RenderConfirm))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.start_render(window, cx);
                            })),
                    ),
            )
            .into_any_element()
    }

    /// Whether the user can choose `target` now: checked, nothing blocks
    /// it, and no render is running.
    fn can_choose(review: &RenderReview, target: &RenderTarget, job: Option<&Job>) -> bool {
        review.is_checked() && target.can_render() && !is_running(job)
    }

    fn include_box(
        &self,
        id: impl Into<gpui_kit::ElementId>,
        target: &RenderTarget,
        label: Option<SharedString>,
        cx: &mut Context<Self>,
    ) -> Checkbox {
        let screen = cx.entity().downgrade();
        let account = target.account;
        let mut checkbox = Checkbox::new(id).checked(self.is_chosen(target)).on_click(
            move |checked: &bool, _, cx| {
                let include = *checked;
                let _ = screen.update(cx, |this, cx| this.toggle_target(account, include, cx));
            },
        );
        if let Some(label) = label {
            checkbox = checkbox.label(label);
        }
        checkbox
    }

    /// Every target as a row: the network and handle, the preset, the
    /// state of its checks and of its last file.
    fn render_targets(
        &self,
        review: &RenderReview,
        job: Option<&Job>,
        cx: &mut Context<Self>,
    ) -> Collection {
        let shown = self.shown_target(review).map(|target| target.account);
        let mut collection = Collection::new(CollectionKind::List, "render-targets");
        let mut tiles = Vec::new();
        for target in &review.targets {
            let account = target.account;
            let include = Self::can_choose(review, target, job)
                .then(|| self.include_box(("render-include", tiles.len()), target, None, cx));
            let bardo = self.bardo.read(cx);
            let state = target_state(review, target);
            let mut tile = Tile::new(
                ("render-target", tiles.len()),
                Rc::new(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.selected_target = Some(account);
                    cx.notify();
                })),
            );
            tile.selected = shown == Some(account);
            tile.title = Some(tr(bardo, Text::NetworkName(target.network)));
            tile.text = Some(SharedString::from(format!(
                "{} · {}",
                target.handle,
                bardo.preset_summary(&target.preset)
            )));
            tile.time = Some(tr(bardo, last_state(target)));
            tile.status = Some(state_chip(bardo, state, cx));
            tile.marks = include
                .map(IntoElement::into_any_element)
                .into_iter()
                .collect();
            tile.attention = matches!(state, TargetState::Warnings(_));
            tile.failed = state == TargetState::Blocked;
            tiles.push(tile);
        }
        let bardo = self.bardo.read(cx);
        collection.controls = vec![
            div()
                .text_xs()
                .font_semibold()
                .text_color(look(cx).tokens.text2)
                .child(tr(bardo, Text::RenderTargets))
                .into_any_element(),
        ];
        collection.tiles = tiles;
        collection.empty = Some(muted(cx, tr(bardo, Text::RenderNoAccounts)));
        collection
    }

    /// The picked target: whether it renders, its preset, encoder and
    /// checks, and its last file.
    fn target_inspector(
        &self,
        review: &RenderReview,
        target: &RenderTarget,
        job: Option<&Job>,
        cx: &mut Context<Self>,
    ) -> Inspector {
        let include = Self::can_choose(review, target, job).then(|| {
            let label = tr(self.bardo.read(cx), Text::RenderInclude);
            self.include_box("render-include-shown", target, Some(label), cx)
                .into_any_element()
        });
        let bardo = self.bardo.read(cx);
        let t = look(cx).tokens;
        let state = target_state(review, target);
        let title = h_flex()
            .gap_2()
            .items_center()
            .child(div().font_semibold().child(SharedString::from(format!(
                "{} · {}",
                bardo.text(Text::NetworkName(target.network)),
                target.handle
            ))))
            .child(state_chip(bardo, state, cx))
            .into_any_element();

        let section = |label: Text, body: Vec<AnyElement>| {
            v_flex()
                .gap_1()
                .child(field_label(tr(bardo, label)))
                .children(body)
                .into_any_element()
        };
        let text = |value: String| {
            div()
                .text_sm()
                .child(SharedString::from(value))
                .into_any_element()
        };

        let mut body: Vec<AnyElement> = include.into_iter().collect();
        body.push(section(
            Text::RenderColumnPreset,
            vec![
                kit::well(cx)
                    .child(SharedString::from(bardo.preset_summary(&target.preset)))
                    .into_any_element(),
            ],
        ));
        let encoder = match (review.is_checked(), target.encoder) {
            (false, _) => bardo.text(Text::RenderStateChecking).into_owned(),
            (true, Some(encoder)) => bardo.encoder_text(encoder),
            (true, None) => "—".to_owned(),
        };
        body.push(section(Text::RenderEncoder, vec![text(encoder)]));
        if review.is_checked() {
            let checks = if target.gates.is_empty() {
                vec![
                    kit::status(Tone::Success, tr(bardo, Text::RenderStateReady), cx)
                        .into_any_element(),
                ]
            } else {
                target
                    .gates
                    .iter()
                    .map(|gate| gate_line(bardo, gate, cx))
                    .collect()
            };
            body.push(section(Text::RenderColumnChecks, checks));
        }

        let mut last = vec![
            h_flex()
                .gap_1()
                .items_center()
                .child(match (&target.last, target.last_current) {
                    (None, _) => kit::status(Tone::Neutral, tr(bardo, Text::RenderLastNone), cx),
                    (Some(_), true) => {
                        kit::status(Tone::Success, tr(bardo, Text::RenderLastCurrent), cx)
                    }
                    (Some(_), false) => {
                        kit::status(Tone::Warning, tr(bardo, Text::RenderLastOutdated), cx)
                    }
                })
                .when(target.last.is_some() && !target.last_current, |row| {
                    row.child(kit::info(
                        "render-outdated-info",
                        None,
                        tr(bardo, Text::RenderLastOutdatedHint),
                    ))
                })
                .into_any_element(),
        ];
        let mut footer = None;
        if let Some(render) = &target.last {
            let on_target = render.loudness_on_target().map(|on_target| {
                if on_target {
                    kit::status(Tone::Success, tr(bardo, Text::RenderLoudnessOnTarget), cx)
                } else {
                    kit::status(Tone::Warning, tr(bardo, Text::RenderLoudnessOffTarget), cx)
                }
            });
            last.push(
                h_flex()
                    .gap_2()
                    .items_center()
                    .flex_wrap()
                    .child(
                        div()
                            .text_sm()
                            .text_color(t.text2)
                            .child(SharedString::from(bardo.render_file_line(render))),
                    )
                    .children(on_target)
                    .into_any_element(),
            );
            let path = bardo.render_path(render);
            if path.exists() {
                let reveal = path.clone();
                last.push(
                    h_flex()
                        .child(
                            Button::new("render-show-file")
                                .small()
                                .outline()
                                .label(tr(bardo, Text::RenderShowFile))
                                .on_click(move |_, _, cx| cx.reveal_path(&reveal)),
                        )
                        .into_any_element(),
                );
            }
            footer = Some(
                kit::details(
                    "render-file-details",
                    tr(bardo, Text::Details),
                    vec![
                        SharedString::from(path.display().to_string()),
                        SharedString::from(render.encoder.clone()),
                    ],
                )
                .into_any_element(),
            );
        }
        body.push(section(Text::RenderColumnLast, last));

        let mut inspector = Inspector::new(body);
        inspector.title = Some(title);
        inspector.footer = footer;
        inspector
    }
}

/// Where a target's last file stands.
fn last_state(target: &RenderTarget) -> Text {
    match (&target.last, target.last_current) {
        (None, _) => Text::RenderLastNone,
        (Some(_), true) => Text::RenderLastCurrent,
        (Some(_), false) => Text::RenderLastOutdated,
    }
}
