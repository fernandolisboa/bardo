//! Versions in other languages, on the Script stage: make the video again
//! for a channel in another language or country, open the versions made,
//! and, on a version, what it was made from.

use bardo_app::bardo_domain::{ChannelId, JobState, VideoProjectId};
use bardo_app::{
    BudgetConsent, Control, LocalizationError, Text, TourAnchor, VersionBlock, VersionsView,
};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, ClickEvent, SharedString, Window, div};

use super::{ProjectsScreen, muted};
use crate::guide;
use crate::kit::{self, Tone};
use crate::shell::tr;
use crate::spend::{budget_question, estimate_note};

impl ProjectsScreen {
    pub(super) fn load_versions(&mut self, id: VideoProjectId, cx: &mut Context<Self>) {
        match self.bardo.read(cx).versions(id) {
            Ok(view) => self.versions = Some(view),
            Err(error) => {
                self.versions = None;
                self.versions_error = Some(error.message());
            }
        }
    }

    fn make_version(
        &mut self,
        channel: ChannelId,
        consent: BudgetConsent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.version_ask = None;
        let Some(id) = self.project else {
            return;
        };
        self.versions_error = match self.bardo.read(cx).make_version(id, channel, consent) {
            Ok(_) => None,
            Err(LocalizationError::OverBudget(estimate)) => {
                self.version_ask = Some((channel, estimate));
                None
            }
            Err(error) => Some(error.message()),
        };
        self.load(window, cx);
        cx.notify();
    }

    /// On a version, what it was made from.
    pub(super) fn render_version_of(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let source = self.versions.as_ref()?.source.as_ref()?;
        let bardo = self.bardo.read(cx);
        let line = bardo.text_with(
            Text::VersionOf,
            &[
                ("title", &source.title),
                (
                    "language",
                    &bardo.text(Text::ContentLanguageName(source.language)),
                ),
                ("country", &bardo.text(Text::CountryName(source.country))),
            ],
        );
        let from = match &source.channel {
            Some(channel) => bardo.text_with(Text::VersionOfChannel, &[("channel", channel)]),
            None => bardo.text(Text::VersionSourceGone).into_owned(),
        };
        let open = source.project.map(|project| {
            Button::new("open-version-source")
                .ghost()
                .xsmall()
                .label(tr(bardo, Text::OpenVersionSource))
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.show_project(project, window, cx)
                }))
        });
        Some(
            kit::notice(Tone::Info, SharedString::from(format!("{line} {from}")), cx)
                .child(
                    h_flex()
                        .gap_1()
                        .flex_wrap()
                        .items_center()
                        .children(open)
                        .child(guide::info(
                            bardo,
                            "version-of-info",
                            tr(bardo, Text::VersionAdaptHint),
                            guide::refs::SCRIPT_VERSIONS,
                        )),
                )
                .into_any_element(),
        )
    }

    /// The running version job, or why the last one stopped.
    fn render_version_job(
        &self,
        view: &VersionsView,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let job = view.job.as_ref()?;
        let bardo = self.bardo.read(cx);
        let channel = view.job_channel.clone().unwrap_or_default();
        match job.state() {
            JobState::Queued | JobState::Running => Some(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(Spinner::new().small())
                    .child(div().text_sm().child(SharedString::from(
                        bardo.text_with(Text::VersionMaking, &[("channel", &channel)]),
                    )))
                    .into_any_element(),
            ),
            JobState::Failed => {
                let failure = job.failure()?;
                Some(
                    v_flex()
                        .gap_1()
                        .child(kit::notice(
                            Tone::Danger,
                            tr(bardo, Text::VersionStopped),
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
                                    "version-failure-details",
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

    /// The Script stage's "Versions in other languages": the channels a
    /// version can be made for, and the versions made.
    pub(super) fn render_versions(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let view = self.versions.as_ref()?;
        let job = self.render_version_job(view, cx);
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let busy = view.is_busy();

        let title_row = h_flex()
            .gap_1()
            .items_center()
            .child(kit::section_heading(tr(bardo, Text::VersionsTitle)))
            .child(guide::info(
                bardo,
                "versions-info",
                tr(bardo, Text::VersionsHint),
                guide::refs::SCRIPT_VERSIONS,
            ));
        let blocked = view.blocked.map(|blocked| {
            muted(
                cx,
                tr(
                    bardo,
                    match blocked {
                        VersionBlock::NoNarration => Text::VersionNoNarration,
                        VersionBlock::NoPlan => Text::VersionNoPlan,
                        VersionBlock::NoChannel => Text::VersionNoChannel,
                    },
                ),
            )
        });
        let template = self
            .view
            .as_ref()
            .map(|view| view.template.number)
            .unwrap_or_default();
        let targets: Vec<AnyElement> = view
            .targets
            .iter()
            .map(|target| {
                let language = bardo.text(Text::ContentLanguageName(target.language));
                let country = bardo.text(Text::CountryName(target.country));
                let line = match &target.narrator {
                    Some(narrator) => bardo.text_with(
                        Text::VersionTargetLine,
                        &[
                            ("language", &language),
                            ("country", &country),
                            ("narrator", narrator),
                        ],
                    ),
                    None => bardo.text_with(
                        Text::VersionTargetNoNarrator,
                        &[("language", &language), ("country", &country)],
                    ),
                };
                let channel = target.channel;
                let make = (!busy).then(|| {
                    h_flex()
                        .gap_1()
                        .items_center()
                        .child(
                            Button::new(SharedString::from(format!("make-version-{channel}")))
                                .primary()
                                .small()
                                .label(tr(bardo, Text::MakeVersion))
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    this.make_version(channel, BudgetConsent::Ask, window, cx)
                                })),
                        )
                        .child(guide::info(
                            bardo,
                            SharedString::from(format!("make-version-info-{channel}")),
                            SharedString::from(
                                bardo.text_with(
                                    Text::MakeVersionHint,
                                    &[("n", &template.to_string())],
                                ),
                            ),
                            guide::refs::SCRIPT_VERSIONS,
                        ))
                });
                kit::well(cx)
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .gap_2()
                    .items_center()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(
                                div()
                                    .font_medium()
                                    .child(SharedString::from(target.name.clone())),
                            )
                            .child(muted(cx, SharedString::from(line)))
                            .children(estimate_note(
                                bardo,
                                &target.estimate,
                                Text::EstimateCost,
                                cx,
                            )),
                    )
                    .children(make)
                    .into_any_element()
            })
            .collect();
        let made = (!view.made.is_empty()).then(|| {
            v_flex()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .font_medium()
                        .child(tr(bardo, Text::VersionsMade)),
                )
                .children(view.made.iter().map(|made| {
                    let project = made.project.id;
                    let line = bardo.text_with(
                        Text::VersionMadeLine,
                        &[
                            ("channel", &made.channel),
                            (
                                "language",
                                &bardo.text(Text::ContentLanguageName(made.language)),
                            ),
                            ("country", &bardo.text(Text::CountryName(made.country))),
                        ],
                    );
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .items_center()
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_sm()
                                        .child(SharedString::from(made.project.title.clone())),
                                )
                                .child(muted(cx, SharedString::from(line))),
                        )
                        .child(
                            Button::new(SharedString::from(format!("open-version-{project}")))
                                .outline()
                                .xsmall()
                                .label(tr(bardo, Text::OpenVersion))
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    this.show_project(project, window, cx)
                                })),
                        )
                }))
        });
        let ask = self.version_ask.as_ref().map(|(channel, estimate)| {
            let channel = *channel;
            budget_question(
                "version-budget",
                bardo,
                estimate,
                cx,
                cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.make_version(channel, BudgetConsent::Confirmed, window, cx)
                }),
                cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.version_ask = None;
                    cx.notify();
                }),
            )
        });

        let section = v_flex()
            .id("projects-versions")
            .pt_3()
            .gap_2()
            .border_t_1()
            .border_color(theme.border)
            .child(title_row)
            .children(
                self.versions_error
                    .map(|error| kit::notice(Tone::Danger, tr(bardo, error), cx)),
            )
            .children(job)
            .children(ask)
            .children(blocked)
            .children(targets)
            .children(made);
        Some(
            kit::anchor_in(
                TourAnchor::Control(Control::ScriptVersions),
                section,
                Some(&self.page_scroll),
            )
            .into_any_element(),
        )
    }
}
