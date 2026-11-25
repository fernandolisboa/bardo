//! The Ad angles tab of a product: the ways an ad for it could speak to
//! one channel's audience. It says what the product still lacks, starts
//! a run in which Claude proposes angles and the decision engine ranks
//! them (with its estimate and the budget question), and lists the
//! angles for review: edit, discard, or approve one into an ad project.
//! Rules, storage and the job live in `bardo_app`; this file maps them
//! to the screen.

use std::time::Duration;

use bardo_app::bardo_domain::{
    AdAngleFieldError, AdAngleId, AdFramework, ChannelId, ClaimId, JobKind, JobState, ProductId,
    ThemeStatus, VideoProjectId,
};
use bardo_app::{
    AD_ANGLES_PER_RUN, AdAngleDraft, AdAngleError, AdAngleRow, AdAnglesView, Bardo, BudgetConsent,
    Control, SpendEstimate, Text, TourAnchor,
};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Selectable as _, Sizable as _, StyledExt as _, h_flex,
    v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Entity, EventEmitter, ScrollHandle, SharedString, Subscription,
    Task, Window, div, px,
};

use crate::guide;
use crate::kit::{self, Tone};
use crate::shell::tr;
use crate::spend::{budget_question, estimate_note};
use crate::themes::reason;

/// How often a run in progress is checked.
const POLL_EVERY: Duration = Duration::from_millis(250);

/// How wide the name of a part of an angle is, so the parts line up.
const PART_NAME_WIDTH: f32 = 92.;

/// Below this the tags of an angle wrap under its title.
const ANGLE_TITLE_MIN_WIDTH: f32 = 260.;

/// The user asked to see an ad project: the one an angle started.
pub struct OpenProject(pub VideoProjectId);

impl EventEmitter<OpenProject> for AnglesSection {}

/// An angle being edited: what is not typed in a field, and what the
/// last save refused.
struct AngleEdit {
    id: AdAngleId,
    framework: AdFramework,
    claims: Vec<ClaimId>,
    errors: Vec<AdAngleFieldError>,
}

pub struct AnglesSection {
    bardo: Entity<Bardo>,
    /// The scroll of the form the tab sits in, so a tour finds its parts.
    scroll: ScrollHandle,
    /// The product on show; `None` while creating one.
    product: Option<ProductId>,
    /// The channel picked; `None` shows the first one.
    channel: Option<ChannelId>,
    view: Option<AdAnglesView>,
    load_failed: bool,
    editing: Option<AngleEdit>,
    title: Entity<InputState>,
    angle: Entity<TextareaState>,
    segment: Entity<InputState>,
    hook: Entity<InputState>,
    /// A run past the budget, waiting for the user's answer: whether it
    /// only ranks, and what it would cost.
    ask: Option<(bool, SpendEstimate)>,
    error: Option<Text>,
    /// The ad project the last approval started.
    started: Option<VideoProjectId>,
    revision: u64,
    _poll: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl AnglesSection {
    pub fn new(
        bardo: Entity<Bardo>,
        scroll: ScrollHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let title = cx.new(|cx| InputState::new(window, cx));
        let angle = cx.new(|cx| TextareaState::new(window, cx).auto_grow(2, 8));
        let segment = cx.new(|cx| InputState::new(window, cx));
        let hook = cx.new(|cx| InputState::new(window, cx));
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL_EVERY).await;
                if this.update(cx, |this, cx| this.poll(cx)).is_err() {
                    break;
                }
            }
        });
        let subscriptions = vec![
            cx.subscribe(&title, |this, _, event, cx| this.typed(event, cx)),
            cx.subscribe(&angle, |this, _, event, cx| this.typed(event, cx)),
            cx.subscribe(&segment, |this, _, event, cx| this.typed(event, cx)),
            cx.subscribe(&hook, |this, _, event, cx| this.typed(event, cx)),
        ];
        let revision = bardo.read(cx).jobs_revision();
        Self {
            bardo,
            scroll,
            product: None,
            channel: None,
            view: None,
            load_failed: false,
            editing: None,
            title,
            angle,
            segment,
            hook,
            ask: None,
            error: None,
            started: None,
            revision,
            _poll: poll,
            _subscriptions: subscriptions,
        }
    }

    /// Shows the angles of `product` as they are stored, or the note to
    /// save the product first.
    pub fn show(&mut self, product: Option<ProductId>, cx: &mut Context<Self>) {
        if self.product != product {
            self.product = product;
            self.channel = None;
            self.editing = None;
            self.ask = None;
            self.error = None;
            self.started = None;
        }
        self.load(cx);
        cx.notify();
    }

    fn typed(&mut self, event: &InputEvent, cx: &mut Context<Self>) {
        if matches!(event, InputEvent::Change)
            && let Some(editing) = self.editing.as_mut()
            && !editing.errors.is_empty()
        {
            editing.errors.clear();
            cx.notify();
        }
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let Some(product) = self.product else {
            self.view = None;
            self.load_failed = false;
            return;
        };
        match self.bardo.read(cx).ad_angles(product, self.channel) {
            Ok(view) => {
                self.channel = view.channel;
                // An angle that left the list (discarded, approved) has
                // nothing left to edit.
                if self.editing.as_ref().is_some_and(|editing| {
                    !view.angles.iter().any(|row| {
                        row.angle.id == editing.id && row.angle.status() == ThemeStatus::Suggested
                    })
                }) {
                    self.editing = None;
                }
                self.view = Some(view);
                self.load_failed = false;
            }
            // The channel picked is gone: the first one, then.
            Err(AdAngleError::ChannelNotFound) if self.channel.is_some() => {
                self.channel = None;
                self.load(cx);
            }
            Err(_) => {
                self.view = None;
                self.load_failed = true;
            }
        }
    }

    /// Reloads when a job moved: a run that ended shows its angles.
    fn poll(&mut self, cx: &mut Context<Self>) {
        if self.product.is_none() {
            return;
        }
        let revision = self.bardo.read(cx).jobs_revision();
        if revision != self.revision {
            self.revision = revision;
            self.load(cx);
            cx.notify();
        }
    }

    fn running(&self) -> bool {
        self.view
            .as_ref()
            .and_then(|view| view.job.as_ref())
            .is_some_and(|job| job.state().is_active())
    }

    fn pick_channel(&mut self, channel: ChannelId, cx: &mut Context<Self>) {
        if self.channel == Some(channel) {
            return;
        }
        self.channel = Some(channel);
        self.editing = None;
        self.ask = None;
        self.error = None;
        self.started = None;
        self.load(cx);
        cx.notify();
    }

    /// Starts a run that proposes and ranks angles, or one that only
    /// ranks the angles waiting; past the budget, asks first unless
    /// `consent` says the user already agreed.
    fn suggest(&mut self, rank_only: bool, consent: BudgetConsent, cx: &mut Context<Self>) {
        self.ask = None;
        let (Some(product), Some(channel)) = (self.product, self.channel) else {
            return;
        };
        let bardo = self.bardo.read(cx);
        let started = if rank_only {
            bardo.rank_ad_angles(product, channel, consent)
        } else {
            bardo.suggest_ad_angles(product, channel, consent)
        };
        self.error = match started {
            Ok(_) => None,
            Err(AdAngleError::OverBudget(estimate)) => {
                self.ask = Some((rank_only, estimate));
                None
            }
            Err(error) => Some(error.message()),
        };
        self.started = None;
        self.load(cx);
        cx.notify();
    }

    fn start_edit(&mut self, row: &AdAngleRow, window: &mut Window, cx: &mut Context<Self>) {
        let draft = AdAngleDraft::of(row.angle.idea());
        self.title
            .update(cx, |input, cx| input.set_value(draft.title, window, cx));
        self.angle
            .update(cx, |input, cx| input.set_value(draft.angle, window, cx));
        self.segment
            .update(cx, |input, cx| input.set_value(draft.segment, window, cx));
        self.hook
            .update(cx, |input, cx| input.set_value(draft.hook, window, cx));
        self.editing = Some(AngleEdit {
            id: row.angle.id,
            framework: draft.framework,
            claims: draft.claims,
            errors: Vec::new(),
        });
        self.error = None;
        cx.notify();
    }

    fn save_edit(&mut self, cx: &mut Context<Self>) {
        let Some(editing) = self.editing.as_ref() else {
            return;
        };
        let draft = AdAngleDraft {
            title: self.title.read(cx).value().to_string(),
            angle: self.angle.read(cx).value().to_string(),
            framework: editing.framework,
            segment: self.segment.read(cx).value().to_string(),
            hook: self.hook.read(cx).value().to_string(),
            claims: editing.claims.clone(),
        };
        let saved = self.bardo.read(cx).edit_ad_angle(editing.id, &draft);
        match saved {
            Ok(_) => {
                self.editing = None;
                self.error = None;
            }
            Err(error) if !error.field_errors().is_empty() => {
                if let Some(editing) = self.editing.as_mut() {
                    editing.errors = error.field_errors().to_vec();
                }
            }
            Err(error) => {
                self.editing = None;
                self.error = Some(error.message());
            }
        }
        self.load(cx);
        cx.notify();
    }

    fn discard(&mut self, id: AdAngleId, cx: &mut Context<Self>) {
        self.error = self
            .bardo
            .read(cx)
            .discard_ad_angle(id)
            .err()
            .map(|error| error.message());
        self.started = None;
        self.load(cx);
        cx.notify();
    }

    fn approve(&mut self, id: AdAngleId, cx: &mut Context<Self>) {
        match self.bardo.read(cx).approve_ad_angle(id) {
            Ok(project) => {
                self.error = None;
                self.started = Some(project.id);
            }
            Err(error) => self.error = Some(error.message()),
        }
        self.load(cx);
        cx.notify();
    }

    fn render_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        v_flex()
            .gap_1()
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(kit::title(tr(bardo, Text::AdAnglesTitle)))
                    .child(guide::info(
                        bardo,
                        "ad-angles-info",
                        tr(bardo, Text::AdAnglesInfo),
                        guide::refs::PRODUCTS_ANGLES,
                    )),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(tr(bardo, Text::AdAnglesHint)),
            )
            .into_any_element()
    }

    /// Progress of a running job, or why the last one stopped.
    fn render_job(&self, view: &AdAnglesView, cx: &App) -> Option<AnyElement> {
        let job = view.job.as_ref()?;
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        match job.state() {
            JobState::Queued | JobState::Running => {
                let percent = job.progress().percent();
                let label = match job.kind() {
                    JobKind::AdAngleRanking => {
                        tr(bardo, Text::JobKindName(JobKind::AdAngleRanking))
                    }
                    _ => tr(bardo, Text::AdAnglesRunning),
                };
                Some(
                    v_flex()
                        .gap_1()
                        .child(div().text_sm().child(label))
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    div().flex_1().child(
                                        Progress::new("ad-angles-progress")
                                            .small()
                                            .value(f32::from(percent)),
                                    ),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(SharedString::from(format!("{percent}%"))),
                                ),
                        )
                        .into_any_element(),
                )
            }
            JobState::Failed => {
                let failure = job.failure()?;
                Some(
                    v_flex()
                        .gap_1()
                        .child(kit::notice(
                            Tone::Danger,
                            tr(bardo, Text::AdAnglesStopped),
                            cx,
                        ))
                        .child(
                            h_flex()
                                .gap_1()
                                .child(
                                    div()
                                        .text_xs()
                                        .child(tr(bardo, Text::JobFailureKindName(failure.kind))),
                                )
                                .child(kit::details(
                                    "ad-angles-failure-details",
                                    tr(bardo, Text::Details),
                                    vec![SharedString::from(failure.detail.clone())],
                                )),
                        )
                        .into_any_element(),
                )
            }
            JobState::Cancelled | JobState::Done => None,
        }
    }

    /// The channel the angles are for, and the buttons that start a run.
    fn render_propose(&self, view: &AdAnglesView, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let running = self.running();
        let held = running || self.ask.is_some();
        // One button per channel, so a long list wraps.
        let channels = h_flex()
            .flex_wrap()
            .gap_1()
            .children(view.channels.iter().enumerate().map(|(ix, channel)| {
                let id = channel.id;
                Button::new(("ad-angles-channel", ix))
                    .small()
                    .outline()
                    .label(SharedString::from(channel.details.name().to_owned()))
                    .selected(view.channel == Some(id))
                    .disabled(running)
                    .on_click(
                        cx.listener(move |this, _: &ClickEvent, _, cx| this.pick_channel(id, cx)),
                    )
            }));
        let channel = (!view.channels.is_empty()).then(|| {
            kit::field(
                tr(bardo, Text::AdAnglesChannel),
                Some(guide::info(
                    bardo,
                    "ad-angles-channel-info",
                    tr(bardo, Text::AdAnglesChannelHint),
                    guide::refs::PRODUCTS_ANGLES,
                )),
                channels.into_any_element(),
                None,
            )
        });
        let missing = view.missing.map(|missing| {
            kit::notice(Tone::Warning, tr(bardo, Text::AdAnglesMissing(missing)), cx)
        });
        let ready = view.missing.is_none();
        let buttons = h_flex()
            .gap_2()
            .flex_wrap()
            .items_center()
            .child(
                Button::new("suggest-ad-angles")
                    .small()
                    .primary()
                    .label(tr(bardo, Text::SuggestAdAngles))
                    .loading(running)
                    .disabled(held || !ready)
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.suggest(false, BudgetConsent::Ask, cx)
                    })),
            )
            .when(view.unranked > 0, |row| {
                row.child(
                    Button::new("rank-ad-angles")
                        .small()
                        .outline()
                        .label(tr(bardo, Text::RankAdAngles))
                        .disabled(held)
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.suggest(true, BudgetConsent::Ask, cx)
                        })),
                )
            })
            .child(guide::info(
                bardo,
                "suggest-ad-angles-info",
                SharedString::from(bardo.text_with(
                    Text::SuggestAdAnglesHint,
                    &[
                        ("n", &AD_ANGLES_PER_RUN.to_string()),
                        ("version", &view.template.number.to_string()),
                    ],
                )),
                guide::refs::PRODUCTS_ANGLES,
            ));
        let ask = self.ask.as_ref().map(|(rank_only, estimate)| {
            let rank_only = *rank_only;
            budget_question(
                "ad-angles-budget",
                bardo,
                estimate,
                cx,
                cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.suggest(rank_only, BudgetConsent::Confirmed, cx)
                }),
                cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.ask = None;
                    cx.notify();
                }),
            )
        });
        kit::card(cx)
            .p_4()
            .gap_3()
            .child(kit::section_heading(tr(bardo, Text::AdAnglesProposeTitle)))
            .children(channel)
            .children(missing)
            .child(buttons)
            .children(
                (ready && !running)
                    .then_some(&view.suggest_estimate)
                    .and_then(|estimate| estimate_note(bardo, estimate, Text::EstimateCost, cx)),
            )
            .when(view.unranked > 0, |card| {
                card.child(div().text_xs().text_color(theme.muted_foreground).child(
                    SharedString::from(
                        bardo.text_with(
                            Text::AdAnglesUnranked,
                            &[("n", &view.unranked.to_string())],
                        ),
                    ),
                ))
            })
            .children(
                view.rank_estimate
                    .as_ref()
                    .filter(|_| !running)
                    .and_then(|estimate| {
                        estimate_note(bardo, estimate, Text::AdAnglesRankCost, cx)
                    }),
            )
            .children(ask)
            .children(
                self.error
                    .map(|error| kit::notice(Tone::Danger, tr(bardo, error), cx)),
            )
            .children(self.render_job(view, cx))
            .into_any_element()
    }

    /// The ad project the last approval started, one click away.
    fn render_started(&self, view: &AdAnglesView, cx: &mut Context<Self>) -> Option<AnyElement> {
        let project = self.started?;
        // Only while the angle that started it is on show.
        view.angles
            .iter()
            .find(|row| row.project == Some(project))?;
        let bardo = self.bardo.read(cx);
        Some(
            h_flex()
                .flex_wrap()
                .gap_3()
                .items_center()
                .child(
                    kit::notice(Tone::Success, tr(bardo, Text::AdAngleStarted), cx)
                        .flex_1()
                        .min_w_0(),
                )
                .child(
                    Button::new("open-started-ad-project")
                        .small()
                        .primary()
                        .label(tr(bardo, Text::AdAngleOpenProject))
                        .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                            cx.emit(OpenProject(project))
                        })),
                )
                .into_any_element(),
        )
    }

    /// One part of an angle: its name, then what it says.
    fn part(&self, name: Text, value: AnyElement, cx: &App) -> gpui_kit::Div {
        let bardo = self.bardo.read(cx);
        h_flex()
            .gap_2()
            .items_start()
            .child(
                div()
                    .flex_none()
                    .w(px(PART_NAME_WIDTH))
                    .pt_0p5()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr(bardo, name)),
            )
            .child(div().flex_1().min_w_0().text_sm().child(value))
    }

    fn render_angle(
        &self,
        ix: usize,
        toured: bool,
        row: &AdAngleRow,
        view: &AdAnglesView,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let angle = &row.angle;
        let idea = angle.idea();
        let approved = angle.status() == ThemeStatus::Approved;
        let editing = self
            .editing
            .as_ref()
            .filter(|editing| editing.id == angle.id);

        let tags = h_flex()
            .gap_1()
            .flex_none()
            .flex_wrap()
            .when(approved, |tags| {
                tags.child(kit::status(
                    Tone::Success,
                    tr(bardo, Text::AdAngleApproved),
                    cx,
                ))
            })
            .when(row.sensitive, |tags| {
                tags.child(kit::status(
                    Tone::Warning,
                    tr(bardo, Text::AdAngleSensitive),
                    cx,
                ))
            })
            .children(angle.ranking().map(|ranking| {
                h_flex()
                    .gap_1()
                    .child(Tag::primary().small().child(SharedString::from(format!(
                        "{} {}",
                        bardo.text(Text::ThemePriority),
                        ranking.priority().value()
                    ))))
                    .child(Tag::secondary().small().child(SharedString::from(format!(
                        "{} {}%",
                        bardo.text(Text::ThemeConfidence),
                        ranking.confidence().percent()
                    ))))
            }));

        let header = h_flex()
            .flex_wrap()
            .gap_x_3()
            .gap_y_1()
            .justify_between()
            .items_start()
            .child(
                h_flex()
                    .flex_grow(1.)
                    .flex_basis(px(ANGLE_TITLE_MIN_WIDTH))
                    .gap_2()
                    .min_w_0()
                    .items_start()
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(SharedString::from(format!("{}.", ix + 1))),
                    )
                    .child(
                        v_flex()
                            .gap_0p5()
                            .min_w_0()
                            .child(
                                div()
                                    .font_medium()
                                    .child(SharedString::from(idea.title().to_owned())),
                            )
                            .when(!idea.angle().is_empty(), |text| {
                                text.child(
                                    div()
                                        .text_sm()
                                        .text_color(theme.muted_foreground)
                                        .child(SharedString::from(idea.angle().to_owned())),
                                )
                            }),
                    ),
            )
            .child(tags);

        if let Some(editing) = editing {
            let editor = self.render_editor(editing, view, cx);
            return kit::card(cx)
                .id(("ad-angle", ix))
                .p_3()
                .gap_3()
                .child(header)
                .child(editor)
                .into_any_element();
        }

        let framework = idea.framework();
        let format = v_flex()
            .gap_0p5()
            .child(tr(bardo, Text::AdFrameworkName(framework)))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(tr(bardo, Text::AdFrameworkSteps(framework))),
            );
        let claims = v_flex().gap_1().children(row.claims.iter().map(|claim| {
            match &claim.text {
                Some(text) => h_flex()
                    .flex_wrap()
                    .gap_2()
                    .items_center()
                    .child(div().min_w_0().child(SharedString::from(text.clone())))
                    .when(claim.sensitive, |line| {
                        line.child(kit::status(
                            Tone::Warning,
                            tr(bardo, Text::ClaimSensitive),
                            cx,
                        ))
                    })
                    .into_any_element(),
                // The sheet no longer confirms it.
                None => kit::status(Tone::Warning, tr(bardo, Text::AdAngleClaimGone), cx)
                    .into_any_element(),
            }
        }));
        let parts = v_flex()
            .gap_2()
            .child(self.part(Text::AdAngleFramework, format.into_any_element(), cx))
            .when(!idea.segment().is_empty(), |parts| {
                parts.child(self.part(
                    Text::AdAngleSegment,
                    SharedString::from(idea.segment().to_owned()).into_any_element(),
                    cx,
                ))
            })
            .child(self.part(
                Text::AdAngleHook,
                SharedString::from(idea.hook().to_owned()).into_any_element(),
                cx,
            ))
            .child(self.part(
                Text::AdAngleClaims,
                if row.claims.is_empty() {
                    div()
                        .text_color(theme.muted_foreground)
                        .child(tr(bardo, Text::AdAngleClaimsNone))
                        .into_any_element()
                } else {
                    claims.into_any_element()
                },
                cx,
            ));

        let reasons: AnyElement = match angle.ranking() {
            Some(ranking) => h_flex()
                .flex_wrap()
                .gap_x_6()
                .gap_y_2()
                .child(reason(bardo, cx, Text::AdAngleHookScore, ranking.hook))
                .child(reason(
                    bardo,
                    cx,
                    Text::AdAngleAudienceFit,
                    ranking.audience_fit,
                ))
                .child(reason(bardo, cx, Text::AdAngleClarity, ranking.clarity))
                .child(div().self_end().child(kit::details(
                    ("ad-angle-ranking-details", ix),
                    tr(bardo, Text::Details),
                    vec![SharedString::from(format!(
                        "{} · {}",
                        bardo.text_with(Text::ThemeRankedBy, &[("model", &ranking.model)]),
                        bardo.time_ago(ranking.ranked_at)
                    ))],
                )))
                .into_any_element(),
            None => h_flex()
                .child(kit::status(
                    Tone::Warning,
                    tr(bardo, Text::AdAngleNotRanked),
                    cx,
                ))
                .into_any_element(),
        };

        let actions: AnyElement = if approved {
            h_flex()
                .children(row.project.map(|project| {
                    Button::new(("open-ad-project", ix))
                        .outline()
                        .small()
                        .label(tr(bardo, Text::AdAngleOpenProject))
                        .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                            cx.emit(OpenProject(project))
                        }))
                }))
                .into_any_element()
        } else {
            let id = angle.id;
            let edited = row.clone();
            h_flex()
                .gap_2()
                .flex_wrap()
                .child(
                    Button::new(("approve-ad-angle", ix))
                        .primary()
                        .small()
                        .label(tr(bardo, Text::ApproveAdAngle))
                        .on_click(
                            cx.listener(move |this, _: &ClickEvent, _, cx| this.approve(id, cx)),
                        ),
                )
                .child(
                    Button::new(("edit-ad-angle", ix))
                        .outline()
                        .small()
                        .label(tr(bardo, Text::EditTheme))
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.start_edit(&edited, window, cx)
                        })),
                )
                .child(
                    Button::new(("discard-ad-angle", ix))
                        .ghost()
                        .small()
                        .label(tr(bardo, Text::DiscardTheme))
                        .on_click(
                            cx.listener(move |this, _: &ClickEvent, _, cx| this.discard(id, cx)),
                        ),
                )
                .child(guide::info(
                    bardo,
                    ("approve-ad-angle-info", ix),
                    tr(bardo, Text::ApproveAdAngleHint),
                    guide::refs::PRODUCTS_ANGLE_REVIEW,
                ))
                .into_any_element()
        };

        // One angle's parts are tagged for the tour.
        let scroll = Some(&self.scroll);
        let said = v_flex()
            .gap_3()
            .child(header)
            .child(parts)
            .child(reasons)
            .into_any_element();
        let (said, actions) = if toured {
            (
                kit::anchor_in(
                    TourAnchor::Control(Control::ProductAngleReview),
                    said,
                    scroll,
                )
                .into_any_element(),
                kit::anchor_in(
                    TourAnchor::Control(Control::ProductAngleActions),
                    actions,
                    scroll,
                )
                .into_any_element(),
            )
        } else {
            (said, actions)
        };
        kit::card(cx)
            .id(("ad-angle", ix))
            .p_3()
            .gap_3()
            .child(said)
            .child(actions)
            .into_any_element()
    }

    fn render_editor(
        &self,
        editing: &AngleEdit,
        view: &AdAnglesView,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let error = |fields: &[AdAngleFieldError]| -> Option<AnyElement> {
            let error = editing.errors.iter().find(|e| fields.contains(e))?;
            Some(
                div()
                    .text_xs()
                    .text_color(theme.danger)
                    .child(tr(bardo, Text::AdAngleFieldError(*error)))
                    .into_any_element(),
            )
        };
        let frameworks =
            h_flex()
                .flex_wrap()
                .gap_1()
                .children(
                    AdFramework::ALL
                        .into_iter()
                        .enumerate()
                        .map(|(ix, choice)| {
                            Button::new(("ad-angle-framework", ix))
                                .small()
                                .outline()
                                .label(tr(bardo, Text::AdFrameworkName(choice)))
                                .selected(choice == editing.framework)
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    if let Some(editing) = this.editing.as_mut() {
                                        editing.framework = choice;
                                        cx.notify();
                                    }
                                }))
                        }),
                );
        let framework = v_flex().gap_1().child(frameworks).child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(tr(bardo, Text::AdFrameworkSteps(editing.framework))),
        );
        let full = editing.claims.len() >= bardo_app::bardo_domain::AdAngleIdea::MAX_CLAIMS;
        let claims = v_flex()
            .gap_1()
            .children(view.claims.iter().enumerate().map(|(ix, claim)| {
                let id = claim.id;
                let checked = editing.claims.contains(&id);
                h_flex()
                    .gap_2()
                    .items_start()
                    .child(
                        Checkbox::new(("ad-angle-claim", ix))
                            .small()
                            .checked(checked)
                            .disabled(!checked && full)
                            .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                if let Some(editing) = this.editing.as_mut() {
                                    editing.claims.retain(|claim| *claim != id);
                                    if *checked {
                                        editing.claims.push(id);
                                    }
                                    cx.notify();
                                }
                            })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .child(SharedString::from(claim.text().to_owned())),
                    )
            }));
        v_flex()
            .gap_3()
            .child(kit::field(
                tr(bardo, Text::ThemeTitle),
                None,
                Input::new(&self.title).into_any_element(),
                error(&[
                    AdAngleFieldError::TitleRequired,
                    AdAngleFieldError::TitleTooLong,
                ]),
            ))
            .child(kit::field(
                tr(bardo, Text::ThemeAngle),
                None,
                Textarea::new(&self.angle).into_any_element(),
                error(&[AdAngleFieldError::AngleTooLong]),
            ))
            .child(kit::field(
                tr(bardo, Text::AdAngleFramework),
                None,
                framework.into_any_element(),
                None,
            ))
            .child(kit::field(
                tr(bardo, Text::AdAngleSegment),
                None,
                Input::new(&self.segment).into_any_element(),
                error(&[AdAngleFieldError::SegmentTooLong]),
            ))
            .child(kit::field(
                tr(bardo, Text::AdAngleHook),
                None,
                Input::new(&self.hook).into_any_element(),
                error(&[
                    AdAngleFieldError::HookRequired,
                    AdAngleFieldError::HookTooLong,
                ]),
            ))
            .child(kit::field(
                tr(bardo, Text::AdAngleClaims),
                None,
                claims.into_any_element(),
                None,
            ))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(tr(bardo, Text::AdAngleEditHint)),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("save-ad-angle")
                            .primary()
                            .small()
                            .label(tr(bardo, Text::SaveTheme))
                            .on_click(
                                cx.listener(|this, _: &ClickEvent, _, cx| this.save_edit(cx)),
                            ),
                    )
                    .child(
                        Button::new("cancel-ad-angle")
                            .ghost()
                            .small()
                            .label(tr(bardo, Text::CancelThemeEdit))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.editing = None;
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }
}

impl Render for AnglesSection {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = self.render_header(cx);
        let column = v_flex().max_w(px(720.)).gap_4().child(header);
        if self.product.is_none() {
            let bardo = self.bardo.read(cx);
            return column.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr(bardo, Text::AdAnglesSaveFirst)),
            );
        }
        let Some(view) = self.view.clone() else {
            let bardo = self.bardo.read(cx);
            return column.child(kit::notice(
                Tone::Danger,
                tr(bardo, Text::AdAnglesNotLoaded),
                cx,
            ));
        };
        let scroll = Some(&self.scroll);
        let propose = self.render_propose(&view, cx);
        let started = self.render_started(&view, cx);
        // The tour shows the first angle still waiting for a review, so
        // its actions are the ones the step explains.
        let toured = view
            .angles
            .iter()
            .position(|row| row.angle.status() != ThemeStatus::Approved)
            .unwrap_or(0);
        let angles: Vec<AnyElement> = view
            .angles
            .iter()
            .enumerate()
            .map(|(ix, row)| self.render_angle(ix, ix == toured, row, &view, cx))
            .collect();
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let muted = |text: SharedString| {
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(text)
        };
        let empty = (angles.is_empty() && view.missing.is_none())
            .then(|| muted(tr(bardo, Text::AdAnglesEmpty)));
        let discarded = (view.discarded > 0).then(|| {
            muted(SharedString::from(bardo.text_with(
                Text::AdAnglesDiscarded,
                &[("n", &view.discarded.to_string())],
            )))
            .text_xs()
        });
        column
            .child(kit::anchor_in(
                TourAnchor::Control(Control::ProductAngles),
                propose,
                scroll,
            ))
            .children(started)
            .children(empty)
            .children(angles)
            .children(discarded)
    }
}
