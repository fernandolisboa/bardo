//! Settings › Backup (issue #143): back up to a `.zip` the user saves,
//! the weekly automatic backups, and restoring one after a review. The
//! jobs, the checks and the restart's rules live in `bardo_app`; this
//! view polls the job queue's revision like the Jobs panel, runs file
//! reads off the UI thread, and restarts Bardo once a restore is unpacked.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use bardo_app::bardo_domain::{
    Job, JobId, JobKind, JobState, MediaSize, RestoreOutcome, SavedBackup,
};
use bardo_app::{
    BackupView, Bardo, Control, GuidePlace, RestoreBlock, RestoreReview, SetUpAgain, SetupTarget,
    Text, TourAnchor,
};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::progress::Progress;
use gpui_kit::component::{Disableable as _, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Entity, PathPromptOptions, ScrollHandle, SharedString,
    Subscription, Task, Window, div, px,
};

use crate::appearance::look;
use crate::guide;
use crate::kit::{self, Tone};
use crate::shell::tr;

/// How often the tab checks the job queue for changes, as the Jobs panel
/// does: one atomic load while nothing moves.
const POLL_EVERY: Duration = Duration::from_millis(100);

/// The media a backup would carry, as last measured.
enum MediaState {
    Measuring,
    Known(MediaSize),
    Unknown,
}

pub struct BackupPanel {
    bardo: Entity<Bardo>,
    /// The settings screen's scroll, so the tour brings a part into view.
    scroll: ScrollHandle,
    view: BackupView,
    revision: u64,
    include_media: bool,
    media: MediaState,
    /// The automatic backups, newest first, and how many backups had
    /// finished when they were listed.
    automatic: Vec<SavedBackup>,
    listed_at: usize,
    /// The backup started from this tab: its result shows until another.
    started: Option<JobId>,
    /// A dialog, or reading the chosen backup, in flight.
    working: Option<Task<()>>,
    review: Option<RestoreReview>,
    /// What the last action could not do.
    error: Option<Text>,
    restarting: bool,
    /// Settings › Backup › Settings file, shown before Restore so a
    /// restore's review stays the tab's last part.
    settings_file: Entity<crate::settings_transfer::SettingsFilePanel>,
    _poll: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl BackupPanel {
    pub fn new(
        bardo: Entity<Bardo>,
        scroll: ScrollHandle,
        settings_file: Entity<crate::settings_transfer::SettingsFilePanel>,
        cx: &mut Context<Self>,
    ) -> Self {
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL_EVERY).await;
                if this.update(cx, |this, cx| this.refresh(cx)).is_err() {
                    break;
                }
            }
        });
        // Labels follow the interface language, and the switch the profile.
        let subscriptions = vec![cx.observe(&bardo, |this, _, cx| {
            this.view = this.bardo.read(cx).backup_view();
            cx.notify();
        })];
        let (view, revision) = {
            let bardo = bardo.read(cx);
            (bardo.backup_view(), bardo.jobs_revision())
        };
        let mut panel = Self {
            bardo,
            scroll,
            view,
            revision,
            include_media: true,
            media: MediaState::Measuring,
            automatic: Vec::new(),
            listed_at: 0,
            started: None,
            working: None,
            review: None,
            error: None,
            restarting: false,
            settings_file,
            _poll: poll,
            _subscriptions: subscriptions,
        };
        panel.measure_media(cx);
        panel.list_automatic(cx);
        panel
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let bardo = self.bardo.read(cx);
        if bardo.restart_needed() && !self.restarting {
            // The backup is unpacked: Bardo swaps it in as it starts again.
            self.restarting = true;
            cx.notify();
            cx.restart();
            return;
        }
        let revision = bardo.jobs_revision();
        if revision == self.revision {
            return;
        }
        self.revision = revision;
        self.view = bardo.backup_view();
        let finished = finished_backups(&bardo.jobs());
        if finished != self.listed_at {
            self.list_automatic(cx);
        }
        cx.notify();
    }

    fn measure_media(&mut self, cx: &mut Context<Self>) {
        let check = self.bardo.read(cx).media_size_check();
        cx.spawn(async move |this, cx| {
            let measured = cx
                .background_executor()
                .spawn(async move { check.run() })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.media = match measured {
                    Ok(size) => MediaState::Known(size),
                    Err(error) => {
                        tracing::warn!("could not measure the media: {error}");
                        MediaState::Unknown
                    }
                };
                cx.notify();
            });
        })
        .detach();
    }

    fn list_automatic(&mut self, cx: &mut Context<Self>) {
        let bardo = self.bardo.read(cx);
        self.listed_at = finished_backups(&bardo.jobs());
        let list = bardo.automatic_backups();
        cx.spawn(async move |this, cx| {
            let listed = cx
                .background_executor()
                .spawn(async move { list.run() })
                .await;
            let _ = this.update(cx, |this, cx| {
                match listed {
                    Ok(saved) => this.automatic = saved,
                    Err(error) => tracing::warn!("could not list the automatic backups: {error}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Asks where to save the backup, then starts it.
    fn back_up(&mut self, cx: &mut Context<Self>) {
        let name = bardo_app::backup_file_name(SystemTime::now());
        let chosen = cx.prompt_for_new_path(&bardo_app::backup_folder(), Some(&name));
        self.error = None;
        let media = self.include_media;
        self.working = Some(cx.spawn(async move |this, cx| {
            let chosen = chosen.await;
            let _ = this.update(cx, |this, cx| {
                this.working = None;
                match chosen {
                    Ok(Ok(Some(path))) => match this.bardo.read(cx).start_backup(path, media) {
                        Ok(id) => this.started = Some(id),
                        Err(error) => this.error = Some(error.message()),
                    },
                    // Cancelled.
                    Ok(Ok(None)) | Err(_) => {}
                    Ok(Err(_)) => this.error = Some(Text::FileDialogFailed),
                }
                this.refresh(cx);
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Asks for a backup file, then reads it for the review.
    fn pick_backup(&mut self, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(tr(self.bardo.read(cx), Text::BackupRestoreFromFile)),
        });
        self.error = None;
        self.review = None;
        self.working = Some(cx.spawn(async move |this, cx| {
            match chosen.await {
                Ok(Ok(Some(paths))) => {
                    if let Some(path) = paths.into_iter().next() {
                        let _ = this.update(cx, |this, cx| this.check(path, cx));
                        return;
                    }
                }
                Ok(Ok(None)) | Err(_) => {}
                Ok(Err(_)) => {
                    let _ = this.update(cx, |this, _| this.error = Some(Text::FileDialogFailed));
                }
            }
            let _ = this.update(cx, |this, cx| {
                this.working = None;
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Reads what the backup at `path` holds, for the review.
    fn check(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let check = self.bardo.read(cx).backup_check(path);
        self.error = None;
        self.review = None;
        self.working = Some(cx.spawn(async move |this, cx| {
            let checked = cx
                .background_executor()
                .spawn(async move { check.run() })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.working = None;
                match checked {
                    Ok(review) => {
                        this.review = Some(review);
                        // The review is the tab's last part: bring it up.
                        this.scroll.scroll_to_bottom();
                    }
                    Err(error) => this.error = Some(bardo_app::backup_error_text(&error)),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn restore(&mut self, cx: &mut Context<Self>) {
        let Some(review) = self.review.take() else {
            return;
        };
        if let Err(error) = self.bardo.read(cx).start_restore(&review) {
            self.error = Some(error.message());
            self.review = Some(review);
        }
        self.refresh(cx);
        cx.notify();
    }

    fn cancel_job(&mut self, id: JobId, cx: &mut Context<Self>) {
        if let Err(error) = self.bardo.read(cx).cancel_job(id) {
            self.error = Some(error.message());
        }
        self.refresh(cx);
        cx.notify();
    }

    fn set_auto_backup(&mut self, on: bool, cx: &mut Context<Self>) {
        let saved = self
            .bardo
            .update(cx, |bardo, cx| {
                let saved = bardo.set_auto_backup(on);
                cx.notify();
                saved
            })
            .is_ok();
        self.error = (!saved).then_some(Text::JobNotUpdated);
        cx.notify();
    }

    fn done_with_restore(&mut self, cx: &mut Context<Self>) {
        self.bardo.read(cx).forget_last_restore();
        self.view = self.bardo.read(cx).backup_view();
        cx.notify();
    }

    fn render_after_restore(&self, outcome: &RestoreOutcome, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let t = look(cx).tokens;
        let mut card = kit::card(cx).p_4().gap_3();
        if outcome.succeeded() {
            card = card
                .child(kit::section_heading(tr(bardo, Text::BackupRestoredTitle)))
                .child(kit::notice(
                    Tone::Success,
                    bardo.text_with(
                        Text::BackupRestored,
                        &[("when", &bardo.backup_time_text(outcome.backup_created_at))],
                    ),
                    cx,
                ));
            if self.view.set_up_again.is_empty() {
                card = card.child(
                    div()
                        .text_sm()
                        .text_color(t.text2)
                        .child(tr(bardo, Text::BackupNothingToSetUp)),
                );
            } else {
                let rows = self
                    .view
                    .set_up_again
                    .iter()
                    .enumerate()
                    .map(|(index, item)| {
                        let (label, open): (String, SetUpOpen) = match item {
                            SetUpAgain::Key(provider) => (
                                bardo.text_with(
                                    Text::BackupSetUpKey,
                                    &[("provider", &bardo.text(Text::ProviderName(*provider)))],
                                ),
                                SetUpOpen::Setup(SetupTarget::Key(*provider)),
                            ),
                            SetUpAgain::AppCredentials(network) => (
                                bardo.text_with(
                                    Text::BackupSetUpCredentials,
                                    &[("network", &bardo.text(Text::NetworkName(*network)))],
                                ),
                                SetUpOpen::Setup(SetupTarget::App(*network)),
                            ),
                            SetUpAgain::Reconnect {
                                network, handle, ..
                            } => (
                                bardo.text_with(
                                    Text::BackupSetUpReconnect,
                                    &[
                                        ("handle", handle.as_str()),
                                        ("network", &bardo.text(Text::NetworkName(*network))),
                                    ],
                                ),
                                SetUpOpen::Place(GuidePlace::Screen(
                                    bardo_app::Destination::Accounts,
                                )),
                            ),
                        };
                        h_flex()
                            .flex_wrap()
                            .gap_2()
                            .items_center()
                            .justify_between()
                            .child(div().min_w_0().text_sm().child(label))
                            .child(
                                Button::new(("backup-set-up", index))
                                    .xsmall()
                                    .outline()
                                    .label(tr(bardo, Text::BackupSetUpOpen))
                                    .on_click(move |_, window, cx| match open {
                                        SetUpOpen::Setup(target) => {
                                            guide::open_setup(target, window, cx)
                                        }
                                        SetUpOpen::Place(place) => guide::go(place, window, cx),
                                    }),
                            )
                    });
                card = card
                    .child(
                        div()
                            .text_sm()
                            .font_medium()
                            .child(tr(bardo, Text::BackupSetUpAgain)),
                    )
                    .child(v_flex().gap_2().children(rows));
            }
        } else if let Some(folder) = &outcome.kept_aside {
            let reveal = folder.clone();
            card = card
                .child(kit::notice(
                    Tone::Danger,
                    bardo.text_with(
                        Text::BackupKeptAside,
                        &[("folder", &folder.display().to_string())],
                    ),
                    cx,
                ))
                .child(
                    h_flex().child(
                        Button::new("backup-kept-aside")
                            .small()
                            .outline()
                            .label(tr(bardo, Text::BackupOpenFolder))
                            .on_click(move |_, _, cx| cx.open_with_system(&reveal)),
                    ),
                );
        } else {
            card = card.child(kit::notice(
                Tone::Danger,
                tr(bardo, Text::BackupRolledBack),
                cx,
            ));
        }
        card.child(
            h_flex().justify_end().child(
                Button::new("backup-restore-done")
                    .small()
                    .label(tr(bardo, Text::BackupDone))
                    .on_click(
                        cx.listener(|this, _: &ClickEvent, _, cx| this.done_with_restore(cx)),
                    ),
            ),
        )
        .into_any_element()
    }

    fn render_back_up(&self, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let t = look(cx).tokens;
        let body = |text: Text| div().text_sm().text_color(t.text2).child(tr(bardo, text));
        let running = self
            .view
            .backup
            .as_ref()
            .filter(|backup| backup.job.state().is_active());
        let media_line = match &self.media {
            MediaState::Measuring => bardo.text(Text::BackupMediaMeasuring).into_owned(),
            MediaState::Unknown => bardo.text(Text::BackupMediaUnknown).into_owned(),
            MediaState::Known(size) if size.files == 0 => {
                bardo.text(Text::BackupMediaNone).into_owned()
            }
            MediaState::Known(size) => bardo.text_with(
                Text::BackupMediaSize,
                &[
                    ("size", &bardo.size_text(size.bytes)),
                    ("files", &size.files.to_string()),
                ],
            ),
        };
        let media = v_flex()
            .gap_1()
            .child(
                Checkbox::new("backup-include-media")
                    .label(tr(bardo, Text::BackupIncludeMedia))
                    .checked(self.include_media)
                    .disabled(running.is_some())
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        this.include_media = *checked;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .pl(px(24.))
                    .text_xs()
                    .text_color(t.text2)
                    .child(format!(
                        "{} {}",
                        bardo.text(Text::BackupMediaHint),
                        media_line
                    )),
            );
        let action: AnyElement = match running {
            Some(backup) => job_progress(
                bardo,
                &backup.job,
                Text::BackupBackingUp,
                "backup",
                cx.listener({
                    let id = backup.job.id();
                    move |this, _: &ClickEvent, _, cx| this.cancel_job(id, cx)
                }),
                cx,
            ),
            None => h_flex()
                .child(
                    Button::new("backup-back-up")
                        .small()
                        .primary()
                        .label(tr(bardo, Text::BackupBackUp))
                        .disabled(self.working.is_some())
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.back_up(cx))),
                )
                .into_any_element(),
        };
        let result = self
            .view
            .backup
            .as_ref()
            .filter(|backup| Some(backup.job.id()) == self.started)
            .and_then(|backup| match backup.job.state() {
                JobState::Done => {
                    let reveal = backup.to.clone();
                    Some(
                        h_flex()
                            .flex_wrap()
                            .gap_2()
                            .child(kit::notice(
                                Tone::Success,
                                bardo.text_with(
                                    Text::BackupSaved,
                                    &[("path", backup.to.display().to_string().as_str())],
                                ),
                                cx,
                            ))
                            .child(
                                Button::new("backup-show")
                                    .ghost()
                                    .xsmall()
                                    .label(tr(bardo, Text::BackupShow))
                                    .on_click(move |_, _, cx| cx.reveal_path(&reveal)),
                            )
                            .into_any_element(),
                    )
                }
                JobState::Failed => Some(failure(bardo, &backup.job, Text::BackupFailed, cx)),
                _ => None,
            });
        kit::anchor_in(
            TourAnchor::Control(Control::BackupCreate),
            v_flex()
                .gap_2()
                .child(kit::section_heading(tr(bardo, Text::BackupTitle)))
                .child(body(Text::BackupSummary))
                .child(media)
                .child(action)
                .children(result),
            Some(&self.scroll),
        )
        .into_any_element()
    }

    fn render_automatic(&self, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let t = look(cx).tokens;
        let busy = self.view.restore_block.is_some() || self.working.is_some();
        let rows: Vec<AnyElement> = self
            .automatic
            .iter()
            .enumerate()
            .map(|(index, saved)| {
                let path = saved.path.clone();
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .items_center()
                    .justify_between()
                    .py_1()
                    .border_b_1()
                    .border_color(t.frame)
                    .child(div().text_sm().child(bardo.text_with(
                        Text::BackupAutomaticRow,
                        &[
                            ("when", &bardo.backup_time_text(saved.created_at)),
                            ("size", &bardo.size_text(saved.bytes)),
                        ],
                    )))
                    .child(
                        Button::new(("backup-restore-automatic", index))
                            .xsmall()
                            .outline()
                            .label(tr(bardo, Text::BackupRestore))
                            .disabled(busy)
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.check(path.clone(), cx)
                            })),
                    )
                    .into_any_element()
            })
            .collect();
        let folder = self
            .automatic
            .first()
            .and_then(|saved| saved.path.parent().map(PathBuf::from));
        kit::anchor_in(
            TourAnchor::Control(Control::BackupAutomatic),
            v_flex()
                .gap_2()
                .child(kit::section_heading(tr(bardo, Text::BackupAutomaticTitle)))
                .child(
                    Checkbox::new("backup-automatic")
                        .label(tr(bardo, Text::BackupAutomaticSwitch))
                        .checked(self.view.auto_backup)
                        .on_click(cx.listener(|this, checked: &bool, _, cx| {
                            this.set_auto_backup(*checked, cx)
                        })),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(t.text2)
                        .child(tr(bardo, Text::BackupAutomaticHint)),
                )
                .child(if rows.is_empty() {
                    div()
                        .text_sm()
                        .text_color(t.text2)
                        .child(tr(bardo, Text::BackupAutomaticNone))
                        .into_any_element()
                } else {
                    v_flex().children(rows).into_any_element()
                })
                .children(folder.map(|folder| {
                    h_flex().child(
                        Button::new("backup-open-folder")
                            .small()
                            .label(tr(bardo, Text::BackupOpenFolder))
                            .on_click(move |_, _, cx| cx.open_with_system(&folder)),
                    )
                })),
            Some(&self.scroll),
        )
        .into_any_element()
    }

    fn render_review(&self, review: &RestoreReview, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let t = look(cx).tokens;
        let info = &review.info;
        let holds = if info.media {
            bardo.text_with(
                Text::BackupReviewWithMedia,
                &[
                    ("database", &bardo.size_text(info.database_bytes)),
                    ("media", &bardo.size_text(info.media_bytes)),
                    ("files", &info.media_files.to_string()),
                ],
            )
        } else {
            bardo.text_with(
                Text::BackupReviewWithoutMedia,
                &[("database", &bardo.size_text(info.database_bytes))],
            )
        };
        let blocked = self.view.restore_block;
        kit::card(cx)
            .p_4()
            .gap_2()
            .child(
                div()
                    .font_semibold()
                    .child(tr(bardo, Text::BackupReviewTitle)),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(t.text2)
                    .child(review.path.display().to_string()),
            )
            .child(div().text_sm().child(bardo.text_with(
                Text::BackupReviewMade,
                &[
                    ("when", &bardo.backup_time_text(info.created_at)),
                    ("version", info.bardo_version.as_str()),
                ],
            )))
            .child(div().text_sm().child(holds))
            .child(kit::notice(
                Tone::Danger,
                tr(bardo, Text::BackupReviewWarning),
                cx,
            ))
            .child(
                div()
                    .text_xs()
                    .text_color(t.text2)
                    .child(tr(bardo, Text::BackupReviewPosts)),
            )
            .children(blocked.map(|block| {
                kit::notice(
                    Tone::Warning,
                    tr(
                        bardo,
                        match block {
                            RestoreBlock::JobsRunning(_) => Text::RestoreWaitsForJobs,
                            RestoreBlock::Restoring => Text::RestoreRunning,
                            RestoreBlock::OtherWindow => Text::RestoreOtherWindow,
                        },
                    ),
                    cx,
                )
            }))
            .child(
                h_flex()
                    .flex_wrap()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("backup-review-cancel")
                            .small()
                            .label(tr(bardo, Text::BackupCancel))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.review = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("backup-review-restore")
                            .small()
                            .danger()
                            .label(tr(bardo, Text::BackupRestoreAndRestart))
                            .disabled(blocked.is_some())
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.restore(cx))),
                    ),
            )
            .into_any_element()
    }

    fn render_restore(&self, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let t = look(cx).tokens;
        let restore = self.view.restore.as_ref();
        let state: Option<AnyElement> = if self.restarting || self.view.restart_needed {
            Some(kit::notice(Tone::Info, tr(bardo, Text::BackupRestarting), cx).into_any_element())
        } else {
            restore.and_then(|job| match job.state() {
                state if state.is_active() => {
                    let waiting = job.progress().permille() > 950;
                    Some(job_progress(
                        bardo,
                        job,
                        if waiting {
                            Text::BackupWaitingForAgent
                        } else {
                            Text::BackupRestoring
                        },
                        "restore",
                        cx.listener({
                            let id = job.id();
                            move |this, _: &ClickEvent, _, cx| this.cancel_job(id, cx)
                        }),
                        cx,
                    ))
                }
                JobState::Failed => Some(failure(bardo, job, Text::BackupRestoreFailed, cx)),
                _ => None,
            })
        };
        let busy = self.view.restore_block == Some(RestoreBlock::Restoring);
        kit::anchor_in(
            TourAnchor::Control(Control::BackupRestore),
            v_flex()
                .gap_2()
                .child(kit::section_heading(tr(bardo, Text::BackupRestoreTitle)))
                .child(
                    div()
                        .text_sm()
                        .text_color(t.text2)
                        .child(tr(bardo, Text::BackupRestoreSummary)),
                )
                .child(
                    h_flex().child(
                        Button::new("backup-restore-file")
                            .small()
                            .label(tr(bardo, Text::BackupRestoreFromFile))
                            .loading(self.working.is_some() && self.review.is_none())
                            .disabled(busy || self.working.is_some())
                            .on_click(
                                cx.listener(|this, _: &ClickEvent, _, cx| this.pick_backup(cx)),
                            ),
                    ),
                )
                .children(state)
                .children(
                    self.review
                        .as_ref()
                        .map(|review| self.render_review(review, cx)),
                ),
            Some(&self.scroll),
        )
        .into_any_element()
    }
}

/// Where a line of the after-restore list leads.
#[derive(Clone, Copy)]
enum SetUpOpen {
    Setup(SetupTarget),
    Place(GuidePlace),
}

/// Backups that finished, made by hand or automatically.
fn finished_backups(jobs: &[Job]) -> usize {
    jobs.iter()
        .filter(|job| job.kind() == JobKind::Backup && job.state() == JobState::Done)
        .count()
}

/// A running backup or restore: its bar, what it does, and Cancel.
fn job_progress(
    bardo: &Bardo,
    job: &Job,
    label: Text,
    id: &'static str,
    cancel: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let percent = job.progress().percent();
    let t = look(cx).tokens;
    v_flex()
        .gap_1()
        .child(
            div()
                .text_sm()
                .text_color(t.text2)
                .child(bardo.text_with(label, &[("percent", &percent.to_string())])),
        )
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    div().flex_1().child(
                        Progress::new(SharedString::from(format!("{id}-progress")))
                            .small()
                            .value(f32::from(percent)),
                    ),
                )
                .child(
                    Button::new(SharedString::from(format!("{id}-cancel")))
                        .xsmall()
                        .outline()
                        .label(tr(bardo, Text::BackupCancel))
                        .on_click(cancel),
                ),
        )
        .into_any_element()
}

/// A backup or restore that failed, with why.
fn failure(bardo: &Bardo, job: &Job, title: Text, cx: &App) -> AnyElement {
    let t = look(cx).tokens;
    v_flex()
        .gap_1()
        .child(kit::notice(Tone::Danger, tr(bardo, title), cx))
        .children(job.failure().map(|failure| {
            div()
                .pl(px(20.))
                .text_xs()
                .text_color(t.text2)
                .child(format!(
                    "{} {}",
                    bardo.text(Text::JobFailureKindName(failure.kind)),
                    failure.detail
                ))
        }))
        .into_any_element()
}

impl Render for BackupPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let after = self
            .view
            .last_restore
            .clone()
            .map(|outcome| self.render_after_restore(&outcome, cx));
        let error = self
            .error
            .map(|error| kit::notice(Tone::Danger, tr(self.bardo.read(cx), error), cx));
        v_flex()
            .max_w(px(720.))
            .gap_5()
            .children(after)
            .children(error)
            .child(self.render_back_up(cx))
            .child(self.render_automatic(cx))
            .child(self.settings_file.clone())
            .child(self.render_restore(cx))
    }
}
