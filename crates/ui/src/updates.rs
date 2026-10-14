//! Updates (issue #144, ADR-0011): the daily check, the bar over the
//! screens when a newer release is out, and the Updates section of
//! Settings › About. What is checked, downloaded and run, and when
//! installing waits, live in `bardo_app`; this view starts checks off the
//! UI thread, polls the job queue for the download's progress like the
//! Jobs panel, and quits Bardo once the installer has started.

use std::time::Duration;

use bardo_app::bardo_domain::{Job, JobState, Version};
use bardo_app::{
    AvailableUpdate, Bardo, CHECK_EVERY, Control, GuidePlace, Installation, SettingsTab, Text,
    TourAnchor, UpdateFound, UpdateView,
};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::progress::Progress;
use gpui_kit::component::text::{TextView, TextViewStyle};
use gpui_kit::component::{
    Disableable as _, Icon, IconName, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, ElementId, Entity, ScrollHandle, SharedString, Subscription, Task,
    WeakEntity, Window, div, px, rems,
};

use crate::appearance::look;
use crate::guide;
use crate::kit::{self, Tone};
use crate::shell::tr;

/// How long after Bardo opens its first check waits, so it never competes
/// with the start.
const FIRST_CHECK_AFTER: Duration = Duration::from_secs(5);

/// How often the section checks the job queue for changes, as the Jobs
/// panel does: one atomic load while nothing moves.
const POLL_EVERY: Duration = Duration::from_millis(100);

/// Starts a check off the UI thread, unless one is under way, or this copy
/// does not check (by itself: unless the user turned checks off).
pub fn check(bardo: &Entity<Bardo>, by_hand: bool, cx: &mut App) {
    let Some(check) = bardo.update(cx, |bardo, cx| {
        let check = bardo.update_check(by_hand);
        cx.notify();
        check
    }) else {
        return;
    };
    let bardo = bardo.downgrade();
    cx.spawn(async move |cx| {
        let result = cx
            .background_executor()
            .spawn(async move { check.run() })
            .await;
        let _ = bardo.update(cx, |bardo, cx| {
            bardo.record_update_check(result);
            cx.notify();
        });
    })
    .detach();
}

/// Checks shortly after Bardo opens, then every day, for as long as the
/// task is kept.
pub fn check_daily(bardo: WeakEntity<Bardo>, cx: &mut App) -> Task<()> {
    cx.spawn(async move |cx| {
        let mut wait = FIRST_CHECK_AFTER;
        loop {
            cx.background_executor().timer(wait).await;
            let Some(bardo) = bardo.upgrade() else {
                break;
            };
            cx.update(|cx| check(&bardo, false, cx));
            wait = CHECK_EVERY;
        }
    })
}

/// The bar over the screens: the newer version, a way to it, and Later.
pub fn banner(bardo: &Entity<Bardo>, version: &Version, cx: &App) -> AnyElement {
    let t = look(cx).tokens;
    let read = bardo.read(cx);
    let later = bardo.clone();
    h_flex()
        .id("update-banner")
        .w_full()
        .flex_none()
        .flex_wrap()
        .items_center()
        .gap_x_3()
        .gap_y_1p5()
        .px_4()
        .py_1p5()
        .bg(t.selected)
        .border_b(t.border_width)
        .border_color(t.accent_edge)
        .child(
            h_flex()
                .flex_1()
                .min_w(px(200.))
                .gap_2()
                .items_center()
                .child(
                    Icon::new(IconName::Info)
                        .size(px(16.))
                        .text_color(t.accent_text)
                        .flex_none(),
                )
                .child(
                    div()
                        .min_w_0()
                        .text_sm()
                        .font_medium()
                        .text_color(t.text)
                        .child(read.text_with(
                            Text::UpdateAvailable,
                            &[("version", &version.to_string())],
                        )),
                ),
        )
        .child(
            h_flex()
                .gap_2()
                .child(
                    Button::new("update-banner-see")
                        .small()
                        .primary()
                        .label(tr(read, Text::UpdateBannerSee))
                        .on_click(|_, window, cx| {
                            guide::go(GuidePlace::Settings(SettingsTab::About), window, cx)
                        }),
                )
                .child(
                    Button::new("update-banner-later")
                        .small()
                        .ghost()
                        .label(tr(read, Text::UpdateBannerLater))
                        .on_click(move |_, _, cx| {
                            later.update(cx, |bardo, cx| {
                                bardo.put_off_update();
                                cx.notify();
                            })
                        }),
                ),
        )
        .into_any_element()
}

/// Settings › About's Updates section.
pub struct UpdatePanel {
    bardo: Entity<Bardo>,
    /// The settings screen's scroll, so the tour brings it into view.
    scroll: ScrollHandle,
    view: UpdateView,
    revision: u64,
    /// The install review is open.
    review: bool,
    /// The installer is being checked and started.
    installing: bool,
    /// What the last action could not do.
    error: Option<Text>,
    _poll: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl UpdatePanel {
    pub fn new(bardo: Entity<Bardo>, scroll: ScrollHandle, cx: &mut Context<Self>) -> Self {
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL_EVERY).await;
                if this.update(cx, |this, cx| this.refresh(cx)).is_err() {
                    break;
                }
            }
        });
        // A check that ended, the switch, the language.
        let subscriptions = vec![cx.observe(&bardo, |this, _, cx| this.reload(cx))];
        let (view, revision) = {
            let bardo = bardo.read(cx);
            (bardo.update_view(), bardo.jobs_revision())
        };
        Self {
            bardo,
            scroll,
            view,
            revision,
            review: false,
            installing: false,
            error: None,
            _poll: poll,
            _subscriptions: subscriptions,
        }
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        let bardo = self.bardo.read(cx);
        self.revision = bardo.jobs_revision();
        self.view = bardo.update_view();
        if !self
            .view
            .available
            .as_ref()
            .is_some_and(|update| update.ready)
        {
            self.review = false;
        }
        cx.notify();
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.bardo.read(cx).jobs_revision() != self.revision {
            self.reload(cx);
        }
    }

    fn check_now(&mut self, cx: &mut Context<Self>) {
        self.error = None;
        check(&self.bardo, true, cx);
    }

    fn set_check(&mut self, on: bool, cx: &mut Context<Self>) {
        let saved = self.bardo.update(cx, |bardo, cx| {
            let saved = bardo.set_update_check(on).is_ok();
            cx.notify();
            saved
        });
        self.error = (!saved).then_some(Text::JobNotUpdated);
        cx.notify();
    }

    fn download(&mut self, cx: &mut Context<Self>) {
        self.error = self
            .bardo
            .read(cx)
            .start_update_download()
            .err()
            .map(|error| error.message());
        self.reload(cx);
    }

    fn cancel_download(&mut self, job: &Job, cx: &mut Context<Self>) {
        if let Err(error) = self.bardo.read(cx).cancel_job(job.id()) {
            self.error = Some(error.message());
        }
        self.reload(cx);
    }

    fn open_review(&mut self, cx: &mut Context<Self>) {
        self.error = None;
        self.review = true;
        cx.notify();
    }

    /// Checks the file once more off the UI thread, starts the installer
    /// and quits, so the installer can replace Bardo.
    fn install(&mut self, cx: &mut Context<Self>) {
        let install = match self.bardo.read(cx).install_update() {
            Ok(install) => install,
            Err(error) => {
                self.error = Some(error.message());
                cx.notify();
                return;
            }
        };
        self.error = None;
        self.installing = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let started = cx
                .background_executor()
                .spawn(async move { install.run() })
                .await;
            let _ = this.update(cx, |this, cx| match started {
                Ok(version) => {
                    tracing::info!(%version, "Bardo closes for its update");
                    cx.quit();
                }
                Err(error) => {
                    tracing::warn!("the update did not start: {error}");
                    this.installing = false;
                    this.error = Some(error.message());
                    this.reload(cx);
                }
            });
        })
        .detach();
    }

    /// When the last check ran and what it found.
    fn last_check_line(&self, bardo: &Bardo) -> String {
        let Some(last) = self.view.last_check else {
            return bardo.text(Text::UpdatesNever).into_owned();
        };
        let when = bardo.backup_time_text(last.at);
        let text = match last.found {
            UpdateFound::UpToDate => Text::UpdatesUpToDate,
            UpdateFound::Newer => Text::UpdatesFoundNewer,
            UpdateFound::NoRelease => Text::UpdatesNoRelease,
            UpdateFound::Offline => Text::UpdatesOffline,
            UpdateFound::RateLimited(Some(until)) => {
                return bardo.text_with(
                    Text::UpdatesRateLimited,
                    &[("when", &bardo.backup_time_text(until))],
                );
            }
            UpdateFound::RateLimited(None) => {
                return bardo.text(Text::UpdatesRateLimitedLater).into_owned();
            }
            UpdateFound::Failed => Text::UpdatesFailed,
        };
        bardo.text_with(text, &[("when", &when)])
    }

    fn render_available(&self, update: &AvailableUpdate, cx: &mut Context<Self>) -> AnyElement {
        let progress = update
            .downloading()
            .map(|job| self.render_progress(job, cx));
        let review = (self.review && update.ready).then(|| self.render_review(update, cx));
        let bardo = self.bardo.read(cx);
        let t = look(cx).tokens;
        let release = &update.release;
        let version = release.version.to_string();
        let published = release.published_at.map(|at| {
            div().text_sm().text_color(t.text2).child(bardo.text_with(
                Text::UpdatePublished,
                &[
                    ("when", &bardo.backup_time_text(at)),
                    ("current", &self.view.current.to_string()),
                ],
            ))
        });
        let notes = (!release.notes.trim().is_empty()).then(|| {
            v_flex()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .font_medium()
                        .child(tr(bardo, Text::UpdateWhatsNew)),
                )
                .child(
                    div()
                        .id("update-notes")
                        .max_h(px(240.))
                        .overflow_y_scroll()
                        .p_3()
                        .bg(t.sunken)
                        .rounded(t.radius)
                        .child(
                            div().w_full().pr_4().child(
                                TextView::markdown(
                                    ElementId::Name(format!("update-notes-{version}").into()),
                                    release.notes.clone(),
                                )
                                .selectable(true)
                                .style(
                                    TextViewStyle::default()
                                        .paragraph_gap(rems(0.5))
                                        // A note's headings stay under the card's title.
                                        .heading_font_size(|level, _| {
                                            if level <= 2 { px(16.) } else { px(14.) }
                                        }),
                                )
                                .on_link_click(|url, _, _, cx| {
                                    // Release notes are the release author's
                                    // text: only web links open.
                                    if url.starts_with("https://") {
                                        cx.open_url(url);
                                    }
                                }),
                            ),
                        ),
                )
        });
        let page = update.release_page().map(|page| {
            let page = page.to_owned();
            Button::new("update-release-page")
                .small()
                .outline()
                .icon(IconName::ExternalLink)
                .label(tr(bardo, Text::UpdateReleasePage))
                .on_click(move |_, _, cx| cx.open_url(&page))
        });
        let action: Option<AnyElement> = if !update.installable {
            (self.view.installation == Installation::Installer).then(|| {
                kit::notice(Tone::Warning, tr(bardo, Text::UpdateNoInstaller), cx)
                    .into_any_element()
            })
        } else if progress.is_some() {
            progress
        } else if update.ready {
            Some(
                v_flex()
                    .gap_2()
                    .child(kit::notice(Tone::Success, tr(bardo, Text::UpdateReady), cx))
                    .child(
                        h_flex().child(
                            Button::new("update-install")
                                .small()
                                .primary()
                                .label(tr(bardo, Text::UpdateInstall))
                                .disabled(self.review || self.installing)
                                .on_click(
                                    cx.listener(|this, _: &ClickEvent, _, cx| this.open_review(cx)),
                                ),
                        ),
                    )
                    .into_any_element(),
            )
        } else {
            let failed = update.failed_download();
            Some(
                v_flex()
                    .gap_2()
                    .children(failed.map(|job| failure(bardo, job, cx)))
                    .child(
                        h_flex().child(
                            Button::new("update-download")
                                .small()
                                .primary()
                                .label(tr(
                                    bardo,
                                    if failed.is_some() {
                                        Text::UpdateDownloadAgain
                                    } else {
                                        Text::UpdateDownload
                                    },
                                ))
                                .on_click(
                                    cx.listener(|this, _: &ClickEvent, _, cx| this.download(cx)),
                                ),
                        ),
                    )
                    .into_any_element(),
            )
        };
        kit::card(cx)
            .p_4()
            .gap_3()
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div().font_semibold().child(
                            bardo.text_with(Text::UpdateAvailable, &[("version", &version)]),
                        ),
                    )
                    .children(published),
            )
            .children(notes)
            .child(
                h_flex()
                    .flex_wrap()
                    .items_start()
                    .gap_3()
                    .child(div().flex_1().min_w(px(200.)).children(action))
                    .children(page),
            )
            .children(review)
            .into_any_element()
    }

    fn render_progress(&self, job: &Job, cx: &mut Context<Self>) -> AnyElement {
        let cancel = {
            let job = job.clone();
            cx.listener(move |this, _: &ClickEvent, _, cx| this.cancel_download(&job, cx))
        };
        let bardo = self.bardo.read(cx);
        let t = look(cx).tokens;
        let percent = job.progress().percent();
        v_flex()
            .gap_1()
            .child(div().text_sm().text_color(t.text2).child(bardo.text_with(
                Text::UpdateDownloading,
                &[("percent", &percent.to_string())],
            )))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div().flex_1().child(
                            Progress::new("update-download-progress")
                                .small()
                                .value(f32::from(percent)),
                        ),
                    )
                    .child(
                        Button::new("update-download-cancel")
                            .xsmall()
                            .outline()
                            .label(tr(bardo, Text::UpdateCancel))
                            .on_click(cancel),
                    ),
            )
            .into_any_element()
    }

    fn render_review(&self, update: &AvailableUpdate, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let t = look(cx).tokens;
        let block = update.install_block;
        v_flex()
            .gap_2()
            .pt_3()
            .border_t(t.border_width)
            .border_color(t.frame)
            .child(div().font_semibold().child(bardo.text_with(
                Text::UpdateReviewTitle,
                &[("version", &update.release.version.to_string())],
            )))
            .child(div().text_sm().child(tr(bardo, Text::UpdateReviewBody)))
            .child(
                div()
                    .text_xs()
                    .text_color(t.text2)
                    .child(tr(bardo, Text::UpdateReviewAgent)),
            )
            .children(block.map(|block| {
                kit::notice(
                    Tone::Warning,
                    tr(bardo, bardo_app::install_block_text(block)),
                    cx,
                )
            }))
            .children(
                self.installing
                    .then(|| kit::notice(Tone::Info, tr(bardo, Text::UpdateStarting), cx)),
            )
            // What stopped the install shows where it was asked for.
            .children(
                self.error
                    .map(|error| kit::notice(Tone::Danger, tr(bardo, error), cx)),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("update-review-cancel")
                            .small()
                            .label(tr(bardo, Text::UpdateCancel))
                            .disabled(self.installing)
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.review = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("update-review-install")
                            .small()
                            .primary()
                            .label(tr(bardo, Text::UpdateInstallAndRestart))
                            .loading(self.installing)
                            .disabled(block.is_some() || self.installing)
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.install(cx))),
                    ),
            )
            .into_any_element()
    }
}

/// A download that failed, with why.
fn failure(bardo: &Bardo, job: &Job, cx: &App) -> AnyElement {
    let t = look(cx).tokens;
    v_flex()
        .gap_1()
        .child(kit::notice(
            Tone::Danger,
            tr(bardo, Text::UpdateDownloadFailed),
            cx,
        ))
        .children(
            job.failure()
                .filter(|_| job.state() == JobState::Failed)
                .map(|failure| {
                    div()
                        .pl(px(20.))
                        .text_xs()
                        .text_color(t.text2)
                        .child(format!(
                            "{} {}",
                            bardo.text(Text::JobFailureKindName(failure.kind)),
                            failure.detail
                        ))
                }),
        )
        .into_any_element()
}

impl Render for UpdatePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bardo = self.bardo.read(cx);
        let t = look(cx).tokens;
        let body = |text: Text| div().text_sm().text_color(t.text2).child(tr(bardo, text));
        let mut section = v_flex()
            .gap_2()
            .child(kit::section_heading(tr(bardo, Text::UpdatesTitle)));
        if self.view.installation == Installation::Store {
            section = section.child(body(Text::UpdatesStore));
        } else {
            let checking = self.view.checking;
            section = section
                .child(body(Text::UpdatesSummary))
                .child(
                    Checkbox::new("update-check-switch")
                        .label(tr(bardo, Text::UpdatesSwitch))
                        .checked(self.view.check_on)
                        .on_click(
                            cx.listener(|this, checked: &bool, _, cx| this.set_check(*checked, cx)),
                        ),
                )
                .child(
                    h_flex()
                        .flex_wrap()
                        .items_center()
                        .gap_x_3()
                        .gap_y_1()
                        .child(
                            Button::new("update-check-now")
                                .small()
                                .label(tr(
                                    bardo,
                                    if checking {
                                        Text::UpdatesChecking
                                    } else {
                                        Text::UpdatesCheckNow
                                    },
                                ))
                                .loading(checking)
                                .disabled(checking)
                                .on_click(
                                    cx.listener(|this, _: &ClickEvent, _, cx| this.check_now(cx)),
                                ),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .text_sm()
                                .text_color(t.text2)
                                .child(SharedString::from(self.last_check_line(bardo))),
                        ),
                )
                .when(self.view.installation == Installation::Source, |section| {
                    section.child(
                        div()
                            .text_xs()
                            .text_color(t.text2)
                            .child(tr(bardo, Text::UpdatesSource)),
                    )
                });
        }
        let reviewing = self.review
            && self
                .view
                .available
                .as_ref()
                .is_some_and(|update| update.ready);
        let error = self
            .error
            .filter(|_| !reviewing)
            .map(|error| kit::notice(Tone::Danger, tr(bardo, error), cx));
        let available = self
            .view
            .available
            .clone()
            .map(|update| self.render_available(&update, cx));
        kit::anchor_in(
            TourAnchor::Control(Control::AboutUpdates),
            section.children(error).children(available),
            Some(&self.scroll),
        )
    }
}
