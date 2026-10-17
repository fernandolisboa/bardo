//! Settings › Backup › Settings file (issue #162, ADR-0014): export the
//! ticked settings into a password-protected file, and import a file after
//! ticking what comes in. What travels and how a file is sealed live in
//! `bardo_app`; this view runs the slow parts (deriving the key from the
//! password) off the UI thread.
//!
//! Also the bar over the screens that offers importing on a new copy's
//! first start, or the file chosen in the installer.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::SystemTime;

use bardo_app::{
    Bardo, Control, ImportOutcome, ItemEffect, ItemProblem, OpenedFile, SettingsChoice,
    SettingsFileInfo, SettingsGroup, SettingsItem, SettingsOffer, SettingsPassword,
    SettingsTransferError, Text, TourAnchor,
};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{
    Disableable as _, Icon, IconName, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Entity, PathPromptOptions, ScrollHandle, SharedString,
    Subscription, Task, Window, div, px,
};

use crate::appearance::{self, look};
use crate::kit::{self, Tone};
use crate::layout;
use crate::shell::tr;

/// A list of settings and which of them are ticked.
struct Checklist {
    choices: Vec<SettingsChoice>,
    ticked: HashSet<SettingsItem>,
}

impl Checklist {
    fn new(choices: Vec<SettingsChoice>) -> Self {
        let ticked = choices
            .iter()
            .filter(|choice| choice.ticked)
            .map(|choice| choice.item)
            .collect();
        Self { choices, ticked }
    }

    /// A sign-in can be ticked only with its channel.
    fn can_tick(&self, choice: &SettingsChoice) -> bool {
        choice.problem.is_none()
            && choice
                .item
                .needs()
                .is_none_or(|needed| self.ticked.contains(&needed))
    }

    fn set(&mut self, item: SettingsItem, on: bool) {
        if on {
            self.ticked.insert(item);
        } else {
            self.ticked.remove(&item);
            // Its sign-ins go with it.
            self.ticked.retain(|other| other.needs() != Some(item));
        }
    }

    /// The ticked items, in the list's order.
    fn ticked(&self) -> Vec<SettingsItem> {
        self.choices
            .iter()
            .map(|choice| choice.item)
            .filter(|item| self.ticked.contains(item))
            .collect()
    }
}

/// Where importing a file stands.
enum Import {
    Idle,
    /// The file is a settings file; its password comes next.
    Locked {
        path: PathBuf,
        info: SettingsFileInfo,
    },
    Opened {
        file: Box<OpenedFile>,
        list: Checklist,
        not_read: Vec<String>,
    },
    Done(ImportOutcome),
}

pub struct SettingsFilePanel {
    bardo: Entity<Bardo>,
    /// The settings screen's scroll, so the tour brings the part into view.
    scroll: ScrollHandle,
    /// The export list, while open.
    export: Option<Checklist>,
    password: Entity<InputState>,
    again: Entity<InputState>,
    /// Where the last export went.
    saved: Option<PathBuf>,
    import: Import,
    import_password: Entity<InputState>,
    /// A dialog, sealing or opening in flight.
    working: Option<Task<()>>,
    error: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl SettingsFilePanel {
    pub fn new(
        bardo: Entity<Bardo>,
        scroll: ScrollHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // Masked: a password never shows on screen, even while typed.
        let password = cx.new(|cx| InputState::new(window, cx).masked(true));
        let again = cx.new(|cx| InputState::new(window, cx).masked(true));
        let import_password = cx.new(|cx| InputState::new(window, cx).masked(true));
        let mut subscriptions = vec![cx.observe(&bardo, |_, _, cx| cx.notify())];
        for input in [&password, &again, &import_password] {
            subscriptions.push(cx.subscribe(input, |this, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    this.error = None;
                    cx.notify();
                }
            }));
        }
        subscriptions.push(cx.subscribe_in(
            &import_password,
            window,
            |this, _, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.open(window, cx);
                }
            },
        ));
        Self {
            bardo,
            scroll,
            export: None,
            password,
            again,
            saved: None,
            import: Import::Idle,
            import_password,
            working: None,
            error: None,
            _subscriptions: subscriptions,
        }
    }

    fn fail(&mut self, error: &SettingsTransferError, cx: &mut Context<Self>) {
        let bardo = self.bardo.read(cx);
        let text = error.message();
        self.error = Some(if text == Text::SettingsFilePasswordShort {
            bardo
                .text_with(
                    text,
                    &[("count", &bardo_app::MIN_PASSWORD_CHARS.to_string())],
                )
                .into()
        } else {
            tr(bardo, text)
        });
    }

    fn clear(input: &Entity<InputState>, window: &mut Window, cx: &mut Context<Self>) {
        input.update(cx, |input, cx| input.set_value("", window, cx));
    }

    fn start_export(&mut self, cx: &mut Context<Self>) {
        self.export = Some(Checklist::new(self.bardo.read(cx).settings_to_export()));
        self.saved = None;
        self.error = None;
        cx.notify();
    }

    fn cancel_export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.export = None;
        self.error = None;
        Self::clear(&self.password, window, cx);
        Self::clear(&self.again, window, cx);
        cx.notify();
    }

    /// Checks the password, asks where to save, then seals the file.
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(list) = &self.export else {
            return;
        };
        let ticked = list.ticked();
        if ticked.is_empty() {
            self.fail(&SettingsTransferError::NothingChosen, cx);
            cx.notify();
            return;
        }
        let password = SettingsPassword::choose(
            &self.password.read(cx).value(),
            &self.again.read(cx).value(),
        );
        let password = match password {
            Ok(password) => password,
            Err(error) => {
                self.fail(&error.into(), cx);
                cx.notify();
                return;
            }
        };
        let name = bardo_app::settings_file_name(SystemTime::now());
        let chosen = cx.prompt_for_new_path(&bardo_app::backup_folder(), Some(&name));
        self.error = None;
        self.working = Some(cx.spawn_in(window, async move |this, cx| {
            let path = match chosen.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) | Err(_) => {
                    let _ = this.update(cx, |this, cx| {
                        this.working = None;
                        cx.notify();
                    });
                    return;
                }
                Ok(Err(_)) => {
                    let _ = this.update(cx, |this, cx| {
                        this.working = None;
                        this.error = Some(tr(this.bardo.read(cx), Text::FileDialogFailed));
                        cx.notify();
                    });
                    return;
                }
            };
            let Ok(export) = this.update(cx, |this, cx| {
                this.bardo
                    .read(cx)
                    .export_settings(&ticked, password, with_extension(path))
            }) else {
                return;
            };
            let written = match export {
                Ok(export) => {
                    cx.background_executor()
                        .spawn(async move { export.run() })
                        .await
                }
                Err(error) => Err(error),
            };
            let _ = this.update_in(cx, |this, window, cx| {
                this.working = None;
                match written {
                    Ok(path) => {
                        this.saved = Some(path);
                        this.export = None;
                        Self::clear(&this.password, window, cx);
                        Self::clear(&this.again, window, cx);
                    }
                    Err(error) => this.fail(&error, cx),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Asks for a settings file, then checks it is one.
    pub fn pick_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(tr(self.bardo.read(cx), Text::SettingsFileImport)),
        });
        self.error = None;
        self.working = Some(cx.spawn_in(window, async move |this, cx| {
            match chosen.await {
                Ok(Ok(Some(paths))) => {
                    if let Some(path) = paths.into_iter().next() {
                        let _ =
                            this.update_in(cx, |this, window, cx| this.inspect(path, window, cx));
                        return;
                    }
                }
                Ok(Ok(None)) | Err(_) => {}
                Ok(Err(_)) => {
                    let _ = this.update(cx, |this, cx| {
                        this.error = Some(tr(this.bardo.read(cx), Text::FileDialogFailed));
                    });
                }
            }
            let _ = this.update(cx, |this, cx| {
                this.working = None;
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Reads what the file at `path` says before its password.
    pub fn inspect(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.error = None;
        self.import = Import::Idle;
        self.working = Some(cx.spawn_in(window, async move |this, cx| {
            let read = path.clone();
            let inspected = cx
                .background_executor()
                .spawn(async move { bardo_app::inspect_settings_file(&read) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.working = None;
                match inspected {
                    Ok(info) => {
                        this.import = Import::Locked { path, info };
                        // The card sits right above Restore, the tab's end.
                        this.scroll.scroll_to_bottom();
                        // Once the field is on screen, the password goes in it.
                        let input = this.import_password.clone();
                        window.defer(cx, move |window, cx| {
                            input.update(cx, |input, cx| input.focus(window, cx));
                        });
                    }
                    Err(error) => this.fail(&error, cx),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Opens the file with the typed password.
    fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Import::Locked { path, .. } = &self.import else {
            return;
        };
        if self.working.is_some() {
            return;
        }
        let password = SettingsPassword::typed(&self.import_password.read(cx).value());
        let open = self
            .bardo
            .read(cx)
            .open_settings_file(path.clone(), password);
        self.error = None;
        self.working = Some(cx.spawn_in(window, async move |this, cx| {
            let opened = cx
                .background_executor()
                .spawn(async move { open.run() })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.working = None;
                match opened {
                    Ok(file) => {
                        let bardo = this.bardo.read(cx);
                        let list = Checklist::new(bardo.settings_to_import(&file));
                        let not_read = bardo.settings_not_read(&file);
                        this.import = Import::Opened {
                            file: Box::new(file),
                            list,
                            not_read,
                        };
                        Self::clear(&this.import_password, window, cx);
                    }
                    Err(error) => this.fail(&error, cx),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn bring_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Import::Opened { file, list, .. } = &self.import else {
            return;
        };
        let ticked = list.ticked();
        if ticked.is_empty() {
            self.fail(&SettingsTransferError::NothingChosen, cx);
            cx.notify();
            return;
        }
        let outcome = self.bardo.update(cx, |bardo, cx| {
            let outcome = bardo.import_settings(file, &ticked);
            cx.notify();
            outcome
        });
        // The theme and the layout may have come in with the preferences.
        let bardo = self.bardo.read(cx);
        let (theme, chosen) = (bardo.ui_theme(), bardo.ui_layout());
        appearance::follow(theme, appearance::system_mode(window), cx);
        layout::show(chosen, cx);
        // The file's secrets go as the opened file drops.
        self.import = Import::Done(outcome);
        cx.notify();
    }

    fn close_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.import = Import::Idle;
        self.error = None;
        Self::clear(&self.import_password, window, cx);
        cx.notify();
    }

    fn error_notice(&self, cx: &App) -> Option<gpui_kit::Div> {
        self.error
            .clone()
            .map(|error| kit::notice(Tone::Danger, error, cx))
    }

    fn render_list(
        &self,
        id: &'static str,
        list: &Checklist,
        importing: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let t = look(cx).tokens;
        let groups = SettingsGroup::ALL.into_iter().filter_map(|group| {
            let rows: Vec<AnyElement> = list
                .choices
                .iter()
                .enumerate()
                .filter(|(_, choice)| choice.item.group() == group)
                .map(|(index, choice)| {
                    let item = choice.item;
                    let enabled = list.can_tick(choice);
                    let checkbox = Checkbox::new((id, index))
                        .label(choice.label.clone())
                        .checked(enabled && list.ticked.contains(&item))
                        .disabled(!enabled || self.working.is_some())
                        .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                            let list = match &mut this.import {
                                Import::Opened { list, .. } if importing => Some(list),
                                _ if !importing => this.export.as_mut(),
                                _ => None,
                            };
                            if let Some(list) = list {
                                list.set(item, *checked);
                            }
                            this.error = None;
                            cx.notify();
                        }));
                    let note = match (choice.problem, choice.effect) {
                        (Some(problem), _) => Some(kit::status(
                            Tone::Warning,
                            tr(
                                bardo,
                                match problem {
                                    ItemProblem::NotReadable => Text::SettingsFileItemNotRead,
                                    ItemProblem::ModelNotOffered => {
                                        Text::SettingsFileModelNotOffered
                                    }
                                },
                            ),
                            cx,
                        )),
                        (None, Some(ItemEffect::Replaces)) => Some(kit::status(
                            Tone::Accent,
                            tr(bardo, Text::SettingsFileReplaces),
                            cx,
                        )),
                        (None, Some(ItemEffect::Adds)) => Some(kit::status(
                            Tone::Neutral,
                            tr(bardo, Text::SettingsFileAdds),
                            cx,
                        )),
                        (None, None) => None,
                    };
                    h_flex()
                        .flex_wrap()
                        .gap_x_2()
                        .gap_y_1()
                        .items_center()
                        .justify_between()
                        .child(div().min_w_0().child(checkbox))
                        .children(note)
                        .into_any_element()
                })
                .collect();
            if rows.is_empty() {
                return None;
            }
            let hint = (group == SettingsGroup::SignIns).then(|| {
                div()
                    .text_xs()
                    .text_color(t.text2)
                    .child(tr(bardo, Text::SettingsFileSignInsHint))
            });
            Some(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                            .child(tr(bardo, group_name(group))),
                    )
                    .children(hint)
                    .child(v_flex().gap_1().pl(px(4.)).children(rows)),
            )
        });
        v_flex().gap_3().children(groups).into_any_element()
    }

    fn render_export(&self, list: &Checklist, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let t = look(cx).tokens;
        let working = self.working.is_some();
        kit::card(cx)
            .p_4()
            .gap_3()
            .child(
                div()
                    .font_semibold()
                    .child(tr(bardo, Text::SettingsFileExportTitle)),
            )
            .child(self.render_list("settings-export", list, false, cx))
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_3()
                    .child(div().flex_1().min_w(px(200.)).child(kit::field(
                        tr(bardo, Text::SettingsFilePassword),
                        None,
                        Input::new(&self.password).into_any_element(),
                        None,
                    )))
                    .child(div().flex_1().min_w(px(200.)).child(kit::field(
                        tr(bardo, Text::SettingsFilePasswordAgain),
                        None,
                        Input::new(&self.again).into_any_element(),
                        None,
                    ))),
            )
            .child(div().text_xs().text_color(t.text2).child(bardo.text_with(
                Text::SettingsFilePasswordHint,
                &[("count", &bardo_app::MIN_PASSWORD_CHARS.to_string())],
            )))
            .children(self.error_notice(cx))
            .child(
                h_flex()
                    .flex_wrap()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("settings-export-cancel")
                            .small()
                            .label(tr(bardo, Text::BackupCancel))
                            .disabled(working)
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.cancel_export(window, cx)
                            })),
                    )
                    .child(
                        Button::new("settings-export-save")
                            .small()
                            .primary()
                            .label(tr(bardo, Text::SettingsFileSave))
                            .loading(working)
                            .disabled(working)
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.save(window, cx)
                            })),
                    ),
            )
            .into_any_element()
    }

    fn render_import(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let bardo = self.bardo.read(cx);
        let t = look(cx).tokens;
        let working = self.working.is_some();
        let cancel = |id: &'static str| {
            Button::new(id)
                .small()
                .label(tr(bardo, Text::BackupCancel))
                .disabled(working)
                .on_click(
                    cx.listener(|this, _: &ClickEvent, window, cx| this.close_import(window, cx)),
                )
        };
        let made = |path: &PathBuf, info: &SettingsFileInfo| {
            v_flex()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .text_color(t.text2)
                        .child(path.display().to_string()),
                )
                .child(div().text_sm().child(bardo.text_with(
                    Text::SettingsFileMade,
                    &[
                        ("version", info.bardo_version.as_str()),
                        ("when", &bardo.backup_time_text(info.created_at)),
                    ],
                )))
        };
        let card = kit::card(cx).p_4().gap_3();
        let card = match &self.import {
            Import::Idle => return None,
            Import::Locked { path, info } => card
                .child(made(path, info))
                .child(kit::field(
                    tr(bardo, Text::SettingsFilePassword),
                    None,
                    Input::new(&self.import_password).into_any_element(),
                    None,
                ))
                .children(self.error_notice(cx))
                .child(
                    h_flex()
                        .flex_wrap()
                        .justify_end()
                        .gap_2()
                        .child(cancel("settings-import-cancel"))
                        .child(
                            Button::new("settings-import-open")
                                .small()
                                .primary()
                                .label(tr(bardo, Text::SettingsFileOpen))
                                .loading(working)
                                .disabled(working)
                                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                    this.open(window, cx)
                                })),
                        ),
                ),
            Import::Opened {
                file,
                list,
                not_read,
            } => card
                .child(made(&file.path, &file.info))
                .child(
                    div()
                        .font_semibold()
                        .child(tr(bardo, Text::SettingsFileImportTitle)),
                )
                .child(self.render_list("settings-import", list, true, cx))
                .when(!not_read.is_empty(), |card| {
                    card.child(kit::notice(
                        Tone::Warning,
                        bardo.text_with(
                            Text::SettingsFilePartsNotRead,
                            &[("parts", &not_read.join(", "))],
                        ),
                        cx,
                    ))
                })
                .child(
                    div()
                        .text_xs()
                        .text_color(t.text2)
                        .child(tr(bardo, Text::SettingsFileReplaceNote)),
                )
                .children(self.error_notice(cx))
                .child(
                    h_flex()
                        .flex_wrap()
                        .justify_end()
                        .gap_2()
                        .child(cancel("settings-import-cancel"))
                        .child(
                            Button::new("settings-import-bring-in")
                                .small()
                                .primary()
                                .label(tr(bardo, Text::SettingsFileBringIn))
                                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                    this.bring_in(window, cx)
                                })),
                        ),
                ),
            Import::Done(outcome) => {
                let failed = outcome.failed.iter().map(|(choice, why)| {
                    div().pl(px(20.)).text_sm().child(format!(
                        "{}: {}",
                        choice.label,
                        bardo.text(*why)
                    ))
                });
                card.child(kit::notice(
                    Tone::Success,
                    bardo.text_with(
                        Text::SettingsFileBrought,
                        &[("count", &outcome.brought.len().to_string())],
                    ),
                    cx,
                ))
                .when(!outcome.failed.is_empty(), |card| {
                    card.child(kit::notice(
                        Tone::Danger,
                        tr(bardo, Text::SettingsFileNotBrought),
                        cx,
                    ))
                    .children(failed)
                })
                .child(
                    h_flex().justify_end().child(
                        Button::new("settings-import-done")
                            .small()
                            .label(tr(bardo, Text::BackupDone))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.close_import(window, cx)
                            })),
                    ),
                )
            }
        };
        Some(card.into_any_element())
    }
}

/// The file ends in `.bardo-settings`, whatever the dialog kept.
fn with_extension(path: PathBuf) -> PathBuf {
    if path
        .extension()
        .is_some_and(|ext| ext == bardo_app::SETTINGS_FILE_EXTENSION)
    {
        path
    } else {
        let mut name = path.into_os_string();
        name.push(".");
        name.push(bardo_app::SETTINGS_FILE_EXTENSION);
        PathBuf::from(name)
    }
}

fn group_name(group: SettingsGroup) -> Text {
    match group {
        SettingsGroup::Keys => Text::SettingsFileGroupKeys,
        SettingsGroup::AppCredentials => Text::SettingsFileGroupApps,
        SettingsGroup::Preferences => Text::SettingsFileGroupPreferences,
        SettingsGroup::Personas => Text::SettingsFileGroupPersonas,
        SettingsGroup::Channels => Text::SettingsFileGroupChannels,
        SettingsGroup::SignIns => Text::SettingsFileGroupSignIns,
    }
}

impl Render for SettingsFilePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bardo = self.bardo.read(cx);
        let t = look(cx).tokens;
        let working = self.working.is_some();
        let busy = working || self.export.is_some() || !matches!(self.import, Import::Idle);
        // An open card shows its own error by its buttons.
        let error = (self.export.is_none()
            && matches!(self.import, Import::Idle | Import::Done(_)))
        .then(|| self.error_notice(cx))
        .flatten();
        let saved = self.saved.clone().map(|path| {
            let reveal = path.clone();
            h_flex()
                .flex_wrap()
                .gap_2()
                .child(kit::notice(
                    Tone::Success,
                    bardo.text_with(
                        Text::SettingsFileSaved,
                        &[("path", path.display().to_string().as_str())],
                    ),
                    cx,
                ))
                .child(
                    Button::new("settings-export-show")
                        .ghost()
                        .xsmall()
                        .label(tr(bardo, Text::BackupShow))
                        .on_click(move |_, _, cx| cx.reveal_path(&reveal)),
                )
        });
        let export = self
            .export
            .as_ref()
            .map(|list| self.render_export(list, cx));
        let import = self.render_import(cx);
        let bardo = self.bardo.read(cx);
        kit::anchor_in(
            TourAnchor::Control(Control::BackupSettingsFile),
            v_flex()
                .gap_2()
                .child(kit::section_heading(tr(bardo, Text::SettingsFileTitle)))
                .child(
                    div()
                        .text_sm()
                        .text_color(t.text2)
                        .child(tr(bardo, Text::SettingsFileSummary)),
                )
                .child(
                    h_flex()
                        .flex_wrap()
                        .gap_2()
                        .child(
                            Button::new("settings-export")
                                .small()
                                .label(tr(bardo, Text::SettingsFileExport))
                                .disabled(busy)
                                .on_click(
                                    cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.start_export(cx)
                                    }),
                                ),
                        )
                        .child(
                            Button::new("settings-import")
                                .small()
                                .label(tr(bardo, Text::SettingsFileImport))
                                .loading(working && matches!(self.import, Import::Idle))
                                .disabled(busy)
                                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                    this.pick_file(window, cx)
                                })),
                        ),
                )
                .children(error)
                .children(saved)
                .children(export)
                .children(import),
            Some(&self.scroll),
        )
    }
}

/// The bar over the screens offering to import settings: on a new copy's
/// first start, or for the file chosen in the installer.
pub fn banner(
    bardo: &Bardo,
    offer: &SettingsOffer,
    take: impl Fn(&ClickEvent, &mut Window, &mut gpui_kit::App) + 'static,
    dismiss: impl Fn(&ClickEvent, &mut Window, &mut gpui_kit::App) + 'static,
    cx: &gpui_kit::App,
) -> AnyElement {
    let t = look(cx).tokens;
    let text = match &offer.file {
        Some(file) => bardo.text_with(
            Text::SettingsOfferFile,
            &[("file", file.display().to_string().as_str())],
        ),
        None => bardo.text(Text::SettingsOfferFirst).into_owned(),
    };
    h_flex()
        .id("settings-offer")
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
                        .child(text),
                ),
        )
        .child(
            h_flex()
                .gap_2()
                .child(
                    Button::new("settings-offer-not-now")
                        .small()
                        .ghost()
                        .label(tr(bardo, Text::SettingsOfferNotNow))
                        .on_click(dismiss),
                )
                .child(
                    Button::new("settings-offer-open")
                        .small()
                        .primary()
                        .label(tr(bardo, Text::SettingsOfferOpen))
                        .on_click(take),
                ),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_keeps_its_extension() {
        assert_eq!(
            with_extension(PathBuf::from("a/bardo")),
            PathBuf::from("a/bardo.bardo-settings")
        );
        assert_eq!(
            with_extension(PathBuf::from("a/b.bardo-settings")),
            PathBuf::from("a/b.bardo-settings")
        );
    }
}
