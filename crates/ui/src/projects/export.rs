//! The Publish stage of the projects screen (#28): each network's post,
//! ready to upload by hand. Claude writes every network's title,
//! description and tags at once; the picked network opens in the
//! inspector, where the user edits them against the network's limits
//! (live counters, problems as sentences) and copies each field. Export
//! queues one job that writes a folder per network with the rendered file
//! and its metadata file; the stage follows it (progress, cancel, resume).
//! Writing the metadata again replaces edits, so it asks first when there
//! are any.

use std::rc::Rc;

use bardo_app::bardo_domain::{
    Cost, Job, JobState, Network, TagPlacement, TemplateKind, VideoMetadataDraft, VideoProjectId,
    compose,
};
use bardo_app::{
    Bardo, BudgetConsent, ExportBlock, ExportError, ExportTarget, ExportView, Text,
    export_job_networks,
};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, ClickEvent, ClipboardItem, SharedString, Window, div, px};

use super::{ProjectsScreen, PromptShown, clock, muted};
use crate::appearance::look;
use crate::kit::{self, Tone};
use crate::parts::{Collection, CollectionKind, Figure, Inspector, ScreenParts, Tile};
use crate::shell::tr;
use crate::spend::{budget_question, estimate_note};

fn is_running(job: Option<&Job>) -> bool {
    job.is_some_and(|job| job.state().is_active())
}

/// Tags as typed: separated by commas, new lines or `#`.
fn split_tags(text: &str) -> Vec<String> {
    text.split([',', '\n', '#'])
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .map(str::to_owned)
        .collect()
}

/// A network's state at the Publish stage, as a chip.
fn state_chip(bardo: &Bardo, target: &ExportTarget, cx: &App) -> AnyElement {
    match target.block() {
        Some(ExportBlock::NoRender) => {
            kit::status(Tone::Neutral, tr(bardo, Text::ExportStateNoRender), cx)
        }
        Some(ExportBlock::NoMetadata) => {
            kit::status(Tone::Neutral, tr(bardo, Text::ExportStateNoMetadata), cx)
        }
        Some(ExportBlock::Problems) => kit::status(
            Tone::Danger,
            bardo.text_with(
                Text::ExportStateProblems,
                &[("n", &target.problems.len().to_string())],
            ),
            cx,
        ),
        None => kit::status(Tone::Success, tr(bardo, Text::ExportStateReady), cx),
    }
    .into_any_element()
}

/// Where a network's last export stands.
fn last_state(target: &ExportTarget) -> Text {
    match (&target.last, target.last_current) {
        (None, _) => Text::ExportLastNone,
        (Some(_), true) => Text::ExportLastCurrent,
        (Some(_), false) => Text::ExportLastOutdated,
    }
}

/// A field's length against its limit, and whether it is over.
fn counter(bardo: &Bardo, n: usize, limit: usize) -> (String, bool) {
    let text = bardo.text_with(
        Text::MetadataCounter,
        &[("n", &n.to_string()), ("limit", &limit.to_string())],
    );
    (text, n > limit)
}

fn field_label(text: SharedString) -> gpui_kit::Div {
    div().text_xs().font_semibold().child(text)
}

impl ProjectsScreen {
    /// Reads the project's Publish stage and fills the inspector's fields
    /// when the network shown or its stored metadata changed.
    pub(super) fn load_export(
        &mut self,
        id: VideoProjectId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.bardo.read(cx).export_view(id) {
            Ok(view) => {
                self.export_summary = Some(view.summary());
                self.export_view = Some(view);
            }
            Err(error) => {
                // The other stages still show; this one says what failed.
                self.export_summary = Some(Default::default());
                self.export_view = None;
                self.export_error = Some(error.message());
            }
        }
        let placeholder = tr(self.bardo.read(cx), Text::MetadataTagsPlaceholder);
        self.metadata_tags.update(cx, |input, cx| {
            input.set_placeholder(placeholder, window, cx)
        });
        self.fill_metadata(window, cx);
    }

    /// The network in the inspector: the picked one, else the first.
    fn shown_network<'a>(&self, view: &'a ExportView) -> Option<&'a ExportTarget> {
        view.targets
            .iter()
            .find(|target| Some(target.network) == self.selected_network)
            .or(view.targets.first())
    }

    /// Puts the shown network's stored metadata in the fields, unless they
    /// already hold it (typing stays across polls).
    fn fill_metadata(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = self.export_view.as_ref() else {
            return;
        };
        let Some(target) = self.shown_network(view) else {
            return;
        };
        let Some(metadata) = target.metadata.as_ref() else {
            self.metadata_loaded = None;
            return;
        };
        let stored = (target.network, metadata.draft());
        if self.metadata_loaded.as_ref() == Some(&stored) {
            return;
        }
        self.set_metadata_fields(&stored.1, window, cx);
        self.metadata_loaded = Some(stored);
    }

    fn set_metadata_fields(
        &mut self,
        draft: &VideoMetadataDraft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (title, description, tags) = (
            draft.title.clone(),
            draft.description.clone(),
            draft.tags.join(", "),
        );
        self.metadata_title
            .update(cx, |input, cx| input.set_value(title, window, cx));
        self.metadata_description
            .update(cx, |input, cx| input.set_value(description, window, cx));
        self.metadata_tags
            .update(cx, |input, cx| input.set_value(tags, window, cx));
    }

    /// The fields as the user typed them.
    fn metadata_draft(&self, cx: &App) -> VideoMetadataDraft {
        VideoMetadataDraft {
            title: self.metadata_title.read(cx).value().to_string(),
            description: self.metadata_description.read(cx).value().to_string(),
            tags: split_tags(&self.metadata_tags.read(cx).value()),
        }
    }

    /// Whether the fields differ from the stored metadata.
    fn metadata_dirty(&self, cx: &App) -> bool {
        self.metadata_loaded
            .as_ref()
            .is_some_and(|(_, stored)| *stored != self.metadata_draft(cx))
    }

    fn pick_network(&mut self, network: Network, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_network != Some(network) {
            self.selected_network = Some(network);
            self.metadata_loaded = None;
            self.export_notice = None;
            self.fill_metadata(window, cx);
        }
        cx.notify();
    }

    /// Writes the metadata, asking first when it would replace edits.
    fn ask_generate_metadata(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let edited = self.export_view.as_ref().is_some_and(|view| {
            view.targets
                .iter()
                .any(|t| t.metadata.as_ref().is_some_and(|m| m.is_edited()))
        });
        if edited {
            self.confirm_metadata = true;
            cx.notify();
        } else {
            self.generate_metadata(BudgetConsent::Ask, window, cx);
        }
    }

    fn generate_metadata(
        &mut self,
        consent: BudgetConsent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.confirm_metadata = false;
        self.metadata_ask = None;
        self.export_notice = None;
        let Some(id) = self.project else {
            return;
        };
        self.export_error = match self.bardo.read(cx).generate_metadata(id, consent) {
            Ok(_) => None,
            Err(ExportError::OverBudget(estimate)) => {
                self.metadata_ask = Some(estimate);
                None
            }
            Err(error) => Some(error.message()),
        };
        self.load(window, cx);
        cx.notify();
    }

    fn save_metadata(&mut self, network: Network, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.project else {
            return;
        };
        let draft = self.metadata_draft(cx);
        match self.bardo.read(cx).edit_metadata(id, network, draft) {
            Ok(_) => {
                self.export_error = None;
                self.export_notice = Some(Text::MetadataSaved);
            }
            Err(error) => {
                self.export_error = Some(match error {
                    ExportError::Repository(_) => Text::MetadataNotSaved,
                    error => error.message(),
                });
                self.export_notice = None;
            }
        }
        // The fields are refilled with what was stored.
        self.metadata_loaded = None;
        self.load(window, cx);
        cx.notify();
    }

    fn revert_metadata(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.metadata_loaded = None;
        self.export_error = None;
        self.export_notice = None;
        self.fill_metadata(window, cx);
        cx.notify();
    }

    fn copy_text(&mut self, text: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.export_notice = Some(Text::MetadataCopied);
        cx.notify();
    }

    /// Whether `target` is in the export: as the user ticked it, else
    /// when its last export is missing or out of date.
    fn is_exported(&self, target: &ExportTarget) -> bool {
        self.export_choices
            .iter()
            .find(|(network, _)| *network == target.network)
            .map_or(!target.last_current, |(_, chosen)| *chosen)
    }

    /// The networks the export would write: the exportable ones chosen.
    fn chosen_networks(&self) -> Vec<Network> {
        let Some(view) = self.export_view.as_ref() else {
            return Vec::new();
        };
        view.targets
            .iter()
            .filter(|target| target.can_export() && self.is_exported(target))
            .map(|target| target.network)
            .collect()
    }

    fn toggle_network(&mut self, network: Network, include: bool, cx: &mut Context<Self>) {
        self.export_choices.retain(|(n, _)| *n != network);
        self.export_choices.push((network, include));
        cx.notify();
    }

    fn start_export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = self.export_view.as_ref() else {
            return;
        };
        let chosen = self.chosen_networks();
        match self.bardo.read(cx).start_export(view, &chosen) {
            Ok(_) => {
                self.export_choices.clear();
                self.export_error = None;
                self.export_notice = None;
            }
            Err(error) => self.export_error = Some(error.message()),
        }
        self.load(window, cx);
        cx.notify();
    }

    fn export_job_action(&mut self, resume: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(job) = self
            .export_view
            .as_ref()
            .and_then(|v| v.export_job.as_ref())
        else {
            return;
        };
        let bardo = self.bardo.read(cx);
        let result = if resume {
            bardo.retry_job(job.id())
        } else {
            bardo.cancel_job(job.id())
        };
        if result.is_err() {
            self.export_error = Some(Text::ExportNotLoaded);
        }
        self.load(window, cx);
        cx.notify();
    }

    /// The Publish stage: figures, the toolbar, what happened, the
    /// networks, and the picked one's post.
    pub(super) fn export_parts(&self, parts: &mut ScreenParts, cx: &mut Context<Self>) {
        let Some(view) = self.export_view.as_ref() else {
            let bardo = self.bardo.read(cx);
            parts.notices.extend(
                self.export_error.map(|error| {
                    kit::notice(Tone::Danger, tr(bardo, error), cx).into_any_element()
                }),
            );
            return;
        };
        parts.summary = self.export_figures(view, cx);
        parts.toolbar = Some(self.export_toolbar(view, cx));
        parts.notices.extend(self.export_notices(view, cx));
        parts.collection = Some(self.export_targets(view, cx));
        parts.inspector = self
            .shown_network(view)
            .map(|target| self.network_inspector(view, target, cx));
    }

    fn export_figures(&self, view: &ExportView, cx: &App) -> Vec<Figure> {
        let bardo = self.bardo.read(cx);
        let summary = view.summary();
        let total = view.targets.len().to_string();
        let of = |n: usize| {
            bardo.text_with(
                Text::ExportFigureNetworks,
                &[("n", &n.to_string()), ("total", &total)],
            )
        };
        let edited = view
            .targets
            .iter()
            .any(|t| t.metadata.as_ref().is_some_and(|m| m.is_edited()));
        let metadata = match view.generation() {
            None => Text::MetadataStateNone,
            Some(_) if edited => Text::MetadataStateEdited,
            Some(_) => Text::MetadataStateGenerated,
        };
        let mut exported = Figure::new(tr(bardo, Text::ExportFigureExported), of(summary.exported));
        if summary.outdated > 0 {
            exported.tone = Some(Tone::Warning);
        }
        let cost = match view.metadata_cost {
            None => "—".to_owned(),
            Some(Cost::Unpriced) => bardo.text(Text::MetadataCostUnpriced).into_owned(),
            Some(cost) => bardo.money(cost.amount()),
        };
        vec![
            Figure::new(tr(bardo, Text::ExportFigureRendered), of(summary.rendered)),
            Figure::new(tr(bardo, Text::ExportFigureMetadata), tr(bardo, metadata)),
            exported,
            Figure::new(tr(bardo, Text::ExportFigureCost), cost),
        ]
    }

    /// What the stage does: write the metadata (or its spinner), how many
    /// networks are chosen, then export (or the running export's progress
    /// and cancel, or resume).
    fn export_toolbar(&self, view: &ExportView, cx: &mut Context<Self>) -> AnyElement {
        let chosen = self.chosen_networks().len();
        let exporting = is_running(view.export_job.as_ref());
        let writing = is_running(view.metadata_job.as_ref());
        let stopped = view.export_job.as_ref().is_some_and(|job| {
            matches!(job.state(), JobState::Failed | JobState::Cancelled) && job.can_retry()
        });
        let bardo = self.bardo.read(cx);
        let mut row = h_flex().gap_2().flex_wrap().items_center().child(kit::info(
            "export-info",
            None,
            tr(bardo, Text::ExportInfo),
        ));
        if writing {
            row = row
                .child(Spinner::new().small())
                .child(div().text_sm().child(tr(bardo, Text::MetadataRunning)));
        } else if !view.targets.is_empty() && !self.confirm_metadata {
            let generated = view.generation().is_some();
            let template = view.template.number.to_string();
            let hint = bardo.text_with(Text::MetadataGenerateHint, &[("n", &template)]);
            let hint = if generated {
                format!("{} {hint}", bardo.text(Text::MetadataRegenerateHint))
            } else {
                hint
            };
            let button = Button::new("metadata-generate")
                .small()
                .label(tr(
                    bardo,
                    if generated {
                        Text::MetadataRegenerate
                    } else {
                        Text::MetadataGenerate
                    },
                ))
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.ask_generate_metadata(window, cx);
                }));
            row = row
                .child(if generated {
                    button.outline()
                } else {
                    button.primary()
                })
                .child(kit::info("metadata-info", None, hint.into()));
        }
        if !view.exportable().is_empty() && !exporting {
            row = row.child(div().text_sm().text_color(look(cx).tokens.text2).child(
                SharedString::from(bardo.text_with(
                    Text::ExportChosen,
                    &[
                        ("n", &chosen.to_string()),
                        ("total", &view.targets.len().to_string()),
                    ],
                )),
            ));
        }
        row = row.child(div().flex_1());
        if let Some(job) = view.export_job.as_ref().filter(|_| exporting) {
            let (done, total) = export_job_networks(job);
            return row
                .child(div().text_sm().child(SharedString::from(bardo.text_with(
                    Text::ExportRunning,
                    &[
                        ("done", &(done + 1).min(total.max(1)).to_string()),
                        ("total", &total.to_string()),
                    ],
                ))))
                .child(
                    div().w(px(160.)).child(
                        Progress::new("export-progress")
                            .small()
                            .value(job.progress().permille() as f32 / 10.0),
                    ),
                )
                .child(
                    Button::new("export-cancel")
                        .small()
                        .outline()
                        .label(tr(bardo, Text::CancelJob))
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.export_job_action(false, window, cx);
                        })),
                )
                .into_any_element();
        }
        if stopped {
            row = row.child(
                Button::new("export-resume")
                    .small()
                    .outline()
                    .label(tr(bardo, Text::RenderResume))
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.export_job_action(true, window, cx);
                    })),
            );
        }
        if chosen > 0 {
            row = row.child(
                Button::new("export-start")
                    .small()
                    .primary()
                    .label(SharedString::from(
                        bardo.text_with(Text::ExportStart, &[("n", &chosen.to_string())]),
                    ))
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.start_export(window, cx);
                    })),
            );
        }
        row.into_any_element()
    }

    /// What happened (an error, a saved edit, a stopped job), the
    /// questions before writing again, and the disclosure reminder.
    fn export_notices(&self, view: &ExportView, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut notices = Vec::new();
        if self.confirm_metadata {
            notices.push(self.metadata_confirm(view, cx));
        }
        if let Some(estimate) = self.metadata_ask.as_ref() {
            notices.push(budget_question(
                "metadata-budget",
                self.bardo.read(cx),
                estimate,
                cx,
                cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.generate_metadata(BudgetConsent::Confirmed, window, cx)
                }),
                cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.metadata_ask = None;
                    cx.notify();
                }),
            ));
        }
        let bardo = self.bardo.read(cx);
        if let Some(notice) = self.export_notice {
            notices.push(kit::notice(Tone::Success, tr(bardo, notice), cx).into_any_element());
        }
        if let Some(error) = self.export_error {
            notices.push(kit::notice(Tone::Danger, tr(bardo, error), cx).into_any_element());
        }
        let failure = |id: &'static str, text: Text, job: &Job| {
            let detail = job
                .failure()
                .map(|failure| SharedString::from(failure.detail.clone()));
            h_flex()
                .gap_2()
                .items_center()
                .child(kit::notice(Tone::Danger, tr(bardo, text), cx))
                .children(
                    detail.map(|detail| kit::details(id, tr(bardo, Text::Details), vec![detail])),
                )
                .into_any_element()
        };
        if let Some(job) = view
            .metadata_job
            .as_ref()
            .filter(|job| job.state() == JobState::Failed)
        {
            notices.push(failure("metadata-failure", Text::MetadataStopped, job));
        }
        match view.export_job.as_ref().map(|job| (job.state(), job)) {
            Some((JobState::Failed, job)) => {
                notices.push(failure("export-failure", Text::ExportStopped, job));
            }
            Some((JobState::Cancelled, _)) => notices.push(
                kit::notice(Tone::Warning, tr(bardo, Text::ExportCancelled), cx).into_any_element(),
            ),
            _ => {}
        }
        if view.disclosure {
            notices.push(
                kit::notice(Tone::Warning, tr(bardo, Text::DisclosureNotice), cx)
                    .into_any_element(),
            );
        }
        if view.generation().is_none()
            && !view.targets.is_empty()
            && !is_running(view.metadata_job.as_ref())
        {
            notices.push(muted(cx, tr(bardo, Text::MetadataEmpty)));
            notices.extend(estimate_note(bardo, &view.estimate, Text::EstimateCost, cx));
        }
        if view.targets.is_empty() {
            notices.push(muted(cx, tr(bardo, Text::ExportNoAccounts)));
        }
        notices
    }

    /// Asks before writing again replaces the user's edits.
    fn metadata_confirm(&self, view: &ExportView, cx: &mut Context<Self>) -> AnyElement {
        let edited = view
            .targets
            .iter()
            .filter(|t| t.metadata.as_ref().is_some_and(|m| m.is_edited()))
            .count();
        let bardo = self.bardo.read(cx);
        let t = look(cx).tokens;
        kit::card(cx)
            .p_4()
            .gap_2()
            .border_color(t.accent_edge)
            .child(
                div()
                    .font_semibold()
                    .child(tr(bardo, Text::MetadataConfirmTitle)),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(t.text2)
                    .child(SharedString::from(bardo.text_with(
                        Text::MetadataConfirmBody,
                        &[("n", &edited.to_string())],
                    ))),
            )
            .children(estimate_note(bardo, &view.estimate, Text::EstimateCost, cx))
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("metadata-confirm-back")
                            .small()
                            .ghost()
                            .label(tr(bardo, Text::RenderConfirmBack))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.confirm_metadata = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("metadata-confirm")
                            .small()
                            .primary()
                            .label(tr(bardo, Text::MetadataRegenerate))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.generate_metadata(BudgetConsent::Ask, window, cx);
                            })),
                    ),
            )
            .into_any_element()
    }

    fn can_pick(view: &ExportView, target: &ExportTarget) -> bool {
        target.can_export() && !is_running(view.export_job.as_ref())
    }

    fn export_box(
        &self,
        id: impl Into<gpui_kit::ElementId>,
        target: &ExportTarget,
        label: Option<SharedString>,
        cx: &mut Context<Self>,
    ) -> Checkbox {
        let screen = cx.entity().downgrade();
        let network = target.network;
        let mut checkbox = Checkbox::new(id)
            .checked(self.is_exported(target))
            .on_click(move |checked: &bool, _, cx| {
                let include = *checked;
                let _ = screen.update(cx, |this, cx| this.toggle_network(network, include, cx));
            });
        if let Some(label) = label {
            checkbox = checkbox.label(label);
        }
        checkbox
    }

    /// Every network as a row: its name and handle, the post's title or
    /// caption, its state and its last export.
    fn export_targets(&self, view: &ExportView, cx: &mut Context<Self>) -> Collection {
        let shown = self.shown_network(view).map(|target| target.network);
        let mut collection = Collection::new(CollectionKind::List, "export-targets");
        let mut tiles = Vec::new();
        for target in &view.targets {
            let network = target.network;
            let include = Self::can_pick(view, target)
                .then(|| self.export_box(("export-include", tiles.len()), target, None, cx));
            let bardo = self.bardo.read(cx);
            let mut tile = Tile::new(
                ("export-target", tiles.len()),
                Rc::new(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.pick_network(network, window, cx);
                })),
            );
            let line = target.metadata.as_ref().map(|metadata| {
                let text = if network.metadata_rules().title.is_some() {
                    metadata.title()
                } else {
                    metadata.description()
                };
                text.lines().next().unwrap_or_default().to_owned()
            });
            tile.selected = shown == Some(network);
            tile.title = Some(tr(bardo, Text::NetworkName(network)));
            tile.text = Some(SharedString::from(match line {
                Some(line) if !line.is_empty() => format!("@{} · {line}", target.handle),
                _ => format!("@{}", target.handle),
            }));
            tile.time = Some(tr(bardo, last_state(target)));
            tile.status = Some(state_chip(bardo, target, cx));
            tile.marks = include
                .map(IntoElement::into_any_element)
                .into_iter()
                .collect();
            tile.attention = target.last.is_some() && !target.last_current;
            tile.failed = target.block() == Some(ExportBlock::Problems);
            tiles.push(tile);
        }
        let bardo = self.bardo.read(cx);
        collection.controls = vec![
            div()
                .text_xs()
                .font_semibold()
                .text_color(look(cx).tokens.text2)
                .child(tr(bardo, Text::ExportTargets))
                .into_any_element(),
        ];
        collection.tiles = tiles;
        collection.empty = Some(muted(cx, tr(bardo, Text::ExportNoAccounts)));
        collection
    }

    /// A field's label with its counter and a copy button beside it.
    fn post_field(
        &self,
        id: &'static str,
        label: Text,
        counter: Option<(String, bool)>,
        copy: Option<String>,
        control: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let t = look(cx).tokens;
        let counter = counter.map(|(text, over)| {
            div()
                .text_xs()
                .text_color(if over { Tone::Danger.ink(cx) } else { t.text2 })
                .child(SharedString::from(text))
        });
        let copy = copy.filter(|text| !text.is_empty()).map(|text| {
            Button::new(id)
                .ghost()
                .xsmall()
                .label(tr(bardo, Text::MetadataCopy))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.copy_text(text.clone(), cx);
                }))
        });
        v_flex()
            .gap_1()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(field_label(tr(bardo, label)))
                    .child(div().flex_1())
                    .children(counter)
                    .children(copy),
            )
            .child(control)
            .into_any_element()
    }

    /// The picked network: whether it exports, its post to edit with live
    /// counters, its render and last export, and where the metadata came
    /// from.
    fn network_inspector(
        &self,
        view: &ExportView,
        target: &ExportTarget,
        cx: &mut Context<Self>,
    ) -> Inspector {
        let network = target.network;
        let include = Self::can_pick(view, target).then(|| {
            let label = tr(self.bardo.read(cx), Text::ExportInclude);
            self.export_box("export-include-shown", target, Some(label), cx)
                .into_any_element()
        });
        let mut body: Vec<AnyElement> = include.into_iter().collect();
        let rules = network.metadata_rules();

        if target.metadata.is_some() {
            let draft = self.metadata_draft(cx);
            let post = compose(network, &draft, &target.footer, target.visibility);
            let problems =
                self.bardo
                    .read(cx)
                    .metadata_draft_problems(network, &draft, &target.footer);
            let count = |text: &str| text.chars().count();
            if let (Some(limit), Some(title)) = (rules.title, post.title.as_ref()) {
                let counter = counter(self.bardo.read(cx), count(title), limit);
                let field = self.post_field(
                    "copy-metadata-title",
                    Text::MetadataFieldTitle,
                    Some(counter),
                    Some(title.clone()),
                    Input::new(&self.metadata_title).into_any_element(),
                    cx,
                );
                body.push(field);
            }
            if let (Some(limit), Some(text)) = (rules.text, post.text.as_ref()) {
                let label = if rules.title.is_some() {
                    Text::MetadataFieldDescription
                } else {
                    Text::MetadataFieldCaption
                };
                let counter = counter(self.bardo.read(cx), count(text), limit);
                let field = self.post_field(
                    "copy-metadata-text",
                    label,
                    Some(counter),
                    Some(text.clone()),
                    Textarea::new(&self.metadata_description).into_any_element(),
                    cx,
                );
                body.push(field);
            }
            let tags = bardo_app::bardo_domain::normalize_tags(network, &draft.tags);
            let tag_counter = {
                let bardo = self.bardo.read(cx);
                match (rules.tags, rules.max_tags, rules.max_tag_chars) {
                    (_, _, Some(limit)) => Some(counter(bardo, count(&post.tag_field()), limit)),
                    (_, Some(limit), None) => Some((
                        bardo.text_with(
                            Text::MetadataTagsCountOf,
                            &[
                                ("n", &tags.len().to_string()),
                                ("limit", &limit.to_string()),
                            ],
                        ),
                        tags.len() > limit,
                    )),
                    _ => Some((
                        bardo.text_with(Text::MetadataTagsCount, &[("n", &tags.len().to_string())]),
                        false,
                    )),
                }
            };
            let tag_copy = (rules.tags == TagPlacement::Field).then(|| post.tag_field());
            let field = self.post_field(
                "copy-metadata-tags",
                Text::MetadataFieldTags,
                tag_counter,
                tag_copy,
                Input::new(&self.metadata_tags).into_any_element(),
                cx,
            );
            body.push(field);

            let dirty = self.metadata_dirty(cx);
            let bardo = self.bardo.read(cx);
            let t = look(cx).tokens;
            // What the footer and the hashtags add, as the network gets it.
            let mut notes = Vec::new();
            if !target.footer.trim().is_empty() && rules.text.is_some() {
                notes.push(tr(bardo, Text::MetadataFooterNote));
            }
            if rules.tags == TagPlacement::Hashtags && rules.text.is_some() {
                notes.push(tr(bardo, Text::MetadataHashtagsNote));
            }
            if !notes.is_empty() {
                let preview = post.text.clone().unwrap_or_default();
                body.push(
                    v_flex()
                        .gap_1()
                        .children(
                            notes
                                .into_iter()
                                .map(|note| div().text_xs().text_color(t.text2).child(note)),
                        )
                        .child(field_label(tr(bardo, Text::MetadataPreview)))
                        .child(kit::well(cx).text_xs().child(SharedString::from(preview)))
                        .into_any_element(),
                );
            }
            body.extend(problems.iter().map(|problem| {
                kit::notice(
                    Tone::Danger,
                    bardo.metadata_problem_text(*problem, network),
                    cx,
                )
                .into_any_element()
            }));
            if dirty {
                body.push(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("save-metadata")
                                .small()
                                .primary()
                                .label(tr(bardo, Text::MetadataSave))
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    this.save_metadata(network, window, cx);
                                })),
                        )
                        .child(
                            Button::new("revert-metadata")
                                .small()
                                .ghost()
                                .label(tr(bardo, Text::MetadataRevert))
                                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                    this.revert_metadata(window, cx);
                                })),
                        )
                        .into_any_element(),
                );
            }
        } else {
            let bardo = self.bardo.read(cx);
            body.push(muted(cx, tr(bardo, Text::MetadataNone)));
        }

        let bardo = self.bardo.read(cx);
        if view.disclosure {
            body.push(
                kit::notice(Tone::Warning, bardo.disclosure_text(network), cx).into_any_element(),
            );
        }

        // The file it exports.
        let render = match &target.render {
            None => vec![muted(cx, tr(bardo, Text::ExportNoRenderHint))],
            Some(render) => {
                let (width, height) = render.preset.dimensions();
                let line = format!(
                    "{width}×{height} · {} · {}",
                    render.preset.aspect.code(),
                    clock(render.duration)
                );
                vec![
                    h_flex()
                        .gap_2()
                        .items_center()
                        .flex_wrap()
                        .child(div().text_sm().child(SharedString::from(line)))
                        .when(!target.render_current, |row| {
                            row.child(kit::status(
                                Tone::Warning,
                                tr(bardo, Text::RenderLastOutdated),
                                cx,
                            ))
                            .child(kit::info(
                                "export-render-outdated",
                                None,
                                tr(bardo, Text::ExportRenderOutdatedHint),
                            ))
                        })
                        .into_any_element(),
                ]
            }
        };
        body.push(
            v_flex()
                .gap_1()
                .child(field_label(tr(bardo, Text::ExportColumnRender)))
                .children(render)
                .into_any_element(),
        );

        // Its last export.
        let mut last = vec![
            h_flex()
                .gap_1()
                .items_center()
                .child(match (&target.last, target.last_current) {
                    (None, _) => kit::status(Tone::Neutral, tr(bardo, Text::ExportLastNone), cx),
                    (Some(_), true) => {
                        kit::status(Tone::Success, tr(bardo, Text::ExportLastCurrent), cx)
                    }
                    (Some(_), false) => {
                        kit::status(Tone::Warning, tr(bardo, Text::ExportLastOutdated), cx)
                    }
                })
                .when(target.last.is_some() && !target.last_current, |row| {
                    row.child(kit::info(
                        "export-outdated-info",
                        None,
                        tr(bardo, Text::ExportLastOutdatedHint),
                    ))
                })
                .into_any_element(),
        ];
        if let Some(export) = &target.last {
            let folder = bardo.export_folder(export);
            let video = folder.join(&export.video_file);
            last.push(
                div()
                    .text_sm()
                    .text_color(look(cx).tokens.text2)
                    .child(SharedString::from(format!(
                        "{} · {}",
                        export.video_file,
                        bardo.time_ago(export.exported_at)
                    )))
                    .into_any_element(),
            );
            let reveal = if video.exists() {
                Some(video)
            } else {
                folder.exists().then(|| folder.clone())
            };
            last.push(
                h_flex()
                    .gap_1()
                    .items_center()
                    .children(reveal.map(|path| {
                        Button::new("export-show-folder")
                            .small()
                            .outline()
                            .label(tr(bardo, Text::ExportShowFolder))
                            .on_click(move |_, _, cx| cx.reveal_path(&path))
                    }))
                    .child(kit::details(
                        "export-folder-details",
                        tr(bardo, Text::Details),
                        vec![SharedString::from(folder.display().to_string())],
                    ))
                    .into_any_element(),
            );
        }
        body.push(
            v_flex()
                .gap_1()
                .child(field_label(tr(bardo, Text::ExportColumnLast)))
                .children(last)
                .into_any_element(),
        );

        let title = h_flex()
            .gap_2()
            .items_center()
            .flex_wrap()
            .child(div().font_semibold().child(SharedString::from(format!(
                "{} · @{}",
                bardo.text(Text::NetworkName(network)),
                target.handle
            ))))
            .child(state_chip(bardo, target, cx))
            .when(
                target.metadata.as_ref().is_some_and(|m| m.is_edited()),
                |row| row.child(kit::status(Tone::Info, tr(bardo, Text::MetadataEdited), cx)),
            )
            .into_any_element();
        let footer = target.metadata.as_ref().map(|metadata| {
            self.render_provenance(
                metadata.generation(),
                TemplateKind::Metadata,
                PromptShown::Metadata,
                cx,
            )
        });
        let mut inspector = Inspector::new(body);
        inspector.title = Some(title);
        inspector.footer = footer;
        inspector
    }
}
