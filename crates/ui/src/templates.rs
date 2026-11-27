//! Templates screen: the prompts Bardo sends to the AI, one template per
//! kind (the script, the scene plan's image prompts). The versions are the
//! collection and the editor its inspector: it holds the current version's
//! text; picking an older version loads its text, and saving always creates
//! the next version. On a project's entry it edits the template that
//! project uses, its own or the global one, and says which; on the Global
//! library's tab it edits the global templates (#339).

use bardo_app::bardo_domain::{
    LibraryItem, LibraryKind, ProjectId, TemplateField, TemplateFieldError, TemplateKind,
    TemplateSource, TemplateVersion, TemplateVersionId,
};
use std::rc::Rc;

use bardo_app::{Bardo, Control, Destination, TemplateError, Text, TourAnchor, default_template};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IconName, Selectable as _, Sizable as _, StyledExt as _,
    h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, ClickEvent, Entity, ScrollHandle, SharedString, Subscription, Window, div, px,
};

use crate::appearance::look;
use crate::kit::{self, Tone};
use crate::library::{self, LibraryScope};
use crate::parts::{Collection, CollectionKind, Header, Inspector, ScreenParts, Tile};
use crate::sharing::{ImportPanel, SharingEvent, SharingPanel};
use crate::shell::tr;
use crate::{guide, layout};

pub struct TemplatesScreen {
    bardo: Entity<Bardo>,
    scope: LibraryScope,
    /// The project whose template the screen edits (`None`: the global
    /// one).
    owner: Option<ProjectId>,
    /// A project entry with no project open.
    no_project: bool,
    kind: TemplateKind,
    /// Whose template of `kind` the owner uses.
    source: TemplateSource,
    /// The projects a save of the global template would change, shown
    /// before it goes through.
    confirm: Option<Vec<String>>,
    /// Newest first; the first is the current version.
    versions: Vec<TemplateVersion>,
    /// The version whose text the editor started from.
    base: Option<TemplateVersionId>,
    instructions: Entity<TextareaState>,
    prompt: Entity<TextareaState>,
    errors: Vec<TemplateFieldError>,
    error: Option<Text>,
    /// What the last save said.
    notice: Option<SharedString>,
    /// Scrolls the editor, so a tour can bring its parts into view.
    scroll: ScrollHandle,
    /// The project's own template's sharing, below the editor.
    sharing: Entity<SharingPanel>,
    /// *Import from another project* and *Open item file…*.
    import: Entity<ImportPanel>,
    _subscriptions: Vec<Subscription>,
}

impl TemplatesScreen {
    pub fn new(
        bardo: Entity<Bardo>,
        scope: LibraryScope,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let instructions = cx.new(|cx| TextareaState::new(window, cx).auto_grow(4, 12));
        let prompt = cx.new(|cx| TextareaState::new(window, cx).auto_grow(6, 16));
        let sharing = cx.new(|_| SharingPanel::new(bardo.clone(), "template-sharing"));
        let import =
            cx.new(|_| ImportPanel::new(bardo.clone(), "template-import", LibraryKind::Template));
        let subscriptions = vec![
            cx.subscribe_in(&sharing, window, |this, _, event, window, cx| {
                this.shared_changed(*event, window, cx)
            }),
            cx.subscribe_in(&import, window, |this, _, event, window, cx| {
                this.shared_changed(*event, window, cx)
            }),
        ];
        let mut screen = Self {
            bardo,
            scope,
            owner: None,
            no_project: false,
            kind: TemplateKind::Script,
            source: TemplateSource::Global,
            confirm: None,
            versions: Vec::new(),
            base: None,
            instructions,
            prompt,
            errors: Vec::new(),
            error: None,
            notice: None,
            scroll: ScrollHandle::new(),
            sharing,
            import,
            _subscriptions: subscriptions,
        };
        screen.reload(window, cx);
        screen
    }

    /// Re-reads the versions and puts the current one in the editor.
    pub fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (owner, no_project) = self.scope.owner(self.bardo.read(cx));
        if owner != self.owner {
            self.notice = None;
        }
        (self.owner, self.no_project) = (owner, no_project);
        self.confirm = None;
        match self.bardo.read(cx).template_in_use(self.kind, self.owner) {
            Ok(in_use) => {
                self.source = in_use.source;
                self.versions = in_use.versions;
                self.error = None;
                if let Some(current) = self.versions.first().cloned() {
                    self.load(&current, window, cx);
                }
            }
            Err(error) => {
                self.versions.clear();
                self.error = Some(match error {
                    bardo_app::TemplateError::Repository(_) => Text::TemplatesNotLoaded,
                    other => other.message(),
                });
            }
        }
        // Only a project's own template takes part in sharing.
        let shown = self
            .owner
            .filter(|_| self.source == TemplateSource::Project)
            .map(|project| (LibraryItem::template(project, self.kind), String::new()));
        self.sharing
            .update(cx, |sharing, cx| sharing.show(shown, cx));
        let project_scope = self.scope == LibraryScope::Project;
        self.import.update(cx, |import, cx| {
            import.set_owner(owner, project_scope, cx);
            import.set_kind(LibraryKind::Template, cx);
        });
        cx.notify();
    }

    /// A sharing change: the template re-reads; an arrived one shows its
    /// kind.
    fn shared_changed(&mut self, event: SharingEvent, window: &mut Window, cx: &mut Context<Self>) {
        if let SharingEvent::Show(LibraryItem::Template { kind, .. }) = event {
            self.kind = kind;
            self.reload(window, cx);
            return;
        }
        // Marking, unlinking or making it global keeps unsaved text.
        let typed = (
            self.instructions.read(cx).value().to_string(),
            self.prompt.read(cx).value().to_string(),
        );
        let edited = self
            .versions
            .iter()
            .find(|version| Some(version.id) == self.base)
            .is_some_and(|base| {
                (base.body.instructions(), base.body.prompt())
                    != (typed.0.as_str(), typed.1.as_str())
            });
        self.reload(window, cx);
        if edited {
            self.fill(typed.0, typed.1, window, cx);
        }
    }

    /// Whether the template has versions to show: the screen offers its
    /// tour only then.
    pub fn has_content(&self) -> bool {
        !self.versions.is_empty()
    }

    fn load(&mut self, version: &TemplateVersion, window: &mut Window, cx: &mut Context<Self>) {
        self.base = Some(version.id);
        self.fill(
            version.body.instructions().to_owned(),
            version.body.prompt().to_owned(),
            window,
            cx,
        );
    }

    fn fill(
        &mut self,
        instructions: String,
        prompt: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.instructions
            .update(cx, |input, cx| input.set_value(instructions, window, cx));
        self.prompt
            .update(cx, |input, cx| input.set_value(prompt, window, cx));
        self.errors.clear();
        cx.notify();
    }

    fn save(&mut self, confirmed: bool, window: &mut Window, cx: &mut Context<Self>) {
        let instructions = self.instructions.read(cx).value();
        let prompt = self.prompt.read(cx).value();
        let current = self.versions.first().map(|version| version.id);
        let result = self.bardo.read(cx).save_template_in(
            self.kind,
            self.owner,
            (&instructions, &prompt),
            confirmed,
        );
        self.confirm = None;
        match result {
            Ok(saved) => {
                let bardo = self.bardo.read(cx);
                self.notice = Some(SharedString::from(if Some(saved.id) == current {
                    bardo.text(Text::TemplateUnchanged).into_owned()
                } else {
                    bardo.text_with(Text::TemplateSaved, &[("n", &saved.number.to_string())])
                }));
                self.reload(window, cx);
            }
            Err(TemplateError::UsedByProjects(projects)) => {
                self.confirm = Some(projects);
                self.notice = None;
            }
            Err(error) if !error.field_errors().is_empty() => {
                self.errors = error.field_errors().to_vec();
                self.notice = None;
            }
            Err(error) => {
                self.error = Some(error.message());
                self.notice = None;
            }
        }
        cx.notify();
    }

    /// Gives the open project its own template of the kind, from the
    /// global one's latest version.
    fn make_own(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self.owner else {
            return;
        };
        let result = self
            .bardo
            .read(cx)
            .make_project_template(self.kind, project);
        match result {
            Ok(_) => {
                self.reload(window, cx);
                self.notice = Some(tr(self.bardo.read(cx), Text::TemplateMadeOwn));
            }
            Err(error) => self.error = Some(error.message()),
        }
        cx.notify();
    }

    /// Whose template the screen edits, on a project's entry, with *Make
    /// it this project's own* while it is the global one.
    fn render_source(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.scope != LibraryScope::Project || self.owner.is_none() || self.versions.is_empty() {
            return None;
        }
        let bardo = self.bardo.read(cx);
        let (pill, hint) = match self.source {
            TemplateSource::Project => (
                kit::status_with(
                    Tone::Accent,
                    IconName::Folder,
                    tr(bardo, Text::TemplateSourceProject),
                    cx,
                ),
                Text::TemplateSourceProjectHint,
            ),
            TemplateSource::Global => (
                kit::status_with(
                    Tone::Info,
                    IconName::Globe,
                    tr(bardo, Text::TemplateSourceGlobal),
                    cx,
                ),
                Text::TemplateSourceGlobalHint,
            ),
        };
        let muted = look(cx).tokens.text2;
        let make_own = (self.source == TemplateSource::Global).then(|| {
            h_flex().child(
                Button::new("template-make-own")
                    .outline()
                    .small()
                    .icon(IconName::Copy)
                    .label(tr(bardo, Text::TemplateMakeOwn))
                    .on_click(
                        cx.listener(|this, _: &ClickEvent, window, cx| this.make_own(window, cx)),
                    ),
            )
        });
        Some(
            kit::anchor(
                TourAnchor::Control(Control::TemplateSource),
                v_flex()
                    .gap_1p5()
                    .child(h_flex().child(pill))
                    .child(div().text_xs().text_color(muted).child(tr(bardo, hint)))
                    .children(make_own),
            )
            .into_any_element(),
        )
    }

    /// The projects a save of the global template changes, with *Save
    /// anyway* and *Cancel*.
    fn render_confirm(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let projects = self.confirm.clone()?;
        let title = tr(self.bardo.read(cx), Text::TemplateGlobalUsedBy);
        let lines = projects
            .iter()
            .map(|name| SharedString::from(format!("• {name}")))
            .collect();
        Some(library::confirm_uses(
            "template-save",
            &self.bardo,
            (title, lines),
            |this: &mut Self, window, cx| this.save(true, window, cx),
            |this: &mut Self, _, cx| {
                this.confirm = None;
                cx.notify();
            },
            cx,
        ))
    }

    /// The template's versions, newest first, under the kind buttons.
    fn collection(&self, cx: &mut Context<Self>) -> Collection {
        let mut collection = Collection::new(CollectionKind::List, "template-versions");
        collection.tiles = self
            .versions
            .iter()
            .enumerate()
            .map(|(ix, version)| {
                let picked = version.clone();
                let mut tile = Tile::new(
                    ("template-version", ix),
                    Rc::new(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.notice = None;
                        this.load(&picked, window, cx);
                    })),
                );
                let bardo = self.bardo.read(cx);
                tile.selected = self.base == Some(version.id);
                tile.title = Some(SharedString::from(bardo.text_with(
                    Text::TemplateVersionLabel,
                    &[("n", &version.number.to_string())],
                )));
                tile.text = Some(SharedString::from(bardo.time_ago(version.created_at)));
                tile.status = (ix == 0).then(|| {
                    kit::status_with(
                        Tone::Accent,
                        IconName::Check,
                        tr(bardo, Text::TemplateCurrent),
                        cx,
                    )
                    .into_any_element()
                });
                tile
            })
            .collect();
        let bardo = self.bardo.read(cx);
        let kinds = TemplateKind::ALL.map(|kind| {
            Button::new(SharedString::from(format!("template-kind-{kind}")))
                .small()
                .outline()
                .selected(kind == self.kind)
                .label(tr(bardo, Text::TemplateKindName(kind)))
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    if this.kind != kind {
                        this.kind = kind;
                        this.notice = None;
                        this.reload(window, cx);
                    }
                }))
        });
        collection.controls = std::iter::once(
            kit::anchor(
                TourAnchor::Control(Control::TemplateKinds),
                h_flex().gap_2().flex_wrap().items_center().children(kinds),
            )
            .into_any_element(),
        )
        .chain(std::iter::once(
            div()
                .w_full()
                .pt_1()
                .text_sm()
                .font_medium()
                .child(tr(bardo, Text::TemplateVersionsTitle))
                .into_any_element(),
        ))
        .collect();
        collection
    }

    fn render_editor(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let source = self.render_source(cx);
        let confirm = self.render_confirm(cx);
        let linked = self.sharing.read(cx).is_linked();
        let shared = self.sharing.read(cx).has_content().then(|| {
            kit::anchor_in(
                TourAnchor::Control(Control::LibrarySharing),
                self.sharing.clone(),
                Some(&self.scroll),
            )
        });
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let base_number = self
            .versions
            .iter()
            .find(|version| Some(version.id) == self.base)
            .map(|version| version.number);
        let next = self
            .versions
            .first()
            .map_or(1, |version| version.number + 1);
        let errors = |field: TemplateField| -> Vec<AnyElement> {
            self.errors
                .iter()
                .filter(|error| error.field == field)
                .map(|error| {
                    kit::notice(
                        Tone::Danger,
                        tr(bardo, Text::TemplateProblem(error.problem)),
                        cx,
                    )
                    .text_xs()
                    .into_any_element()
                })
                .collect()
        };
        let variables: Vec<AnyElement> = self
            .kind
            .variables()
            .iter()
            .map(|variable| {
                h_flex()
                    .gap_2()
                    .items_baseline()
                    .child(
                        div()
                            .w(px(150.))
                            .flex_none()
                            .text_xs()
                            .font_medium()
                            .text_color(look(cx).tokens.accent_text)
                            .child(SharedString::from(variable.placeholder())),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(tr(bardo, Text::TemplateVariableHint(*variable))),
                    )
                    .into_any_element()
            })
            .collect();

        let scroll = Some(&self.scroll);
        v_flex()
            .gap_3()
            .child(kit::section_heading(tr(
                bardo,
                Text::TemplateKindName(self.kind),
            )))
            .children(
                self.no_project
                    .then(|| kit::notice(Tone::Info, tr(bardo, Text::LibraryNoProject), cx)),
            )
            .child(self.import.clone())
            .children(source)
            .children(base_number.map(|n| {
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(SharedString::from(bardo.text_with(
                        Text::TemplateEditing,
                        &[("n", &n.to_string()), ("next", &next.to_string())],
                    )))
            }))
            .child(kit::anchor_in(
                TourAnchor::Control(Control::TemplateFields),
                v_flex()
                    .gap_3()
                    .child(
                        field(
                            bardo,
                            tr(bardo, Text::TemplateInstructions),
                            tr(bardo, Text::TemplateInstructionsHint),
                            Textarea::new(&self.instructions).into_any_element(),
                        )
                        .children(errors(TemplateField::Instructions)),
                    )
                    .child(
                        field(
                            bardo,
                            tr(bardo, Text::TemplatePrompt),
                            tr(bardo, Text::TemplatePromptHint),
                            Textarea::new(&self.prompt).into_any_element(),
                        )
                        .children(errors(TemplateField::Prompt)),
                    ),
                scroll,
            ))
            .child(kit::anchor_in(
                TourAnchor::Control(Control::TemplateActions),
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(
                        Button::new("save-template")
                            .primary()
                            .disabled(self.no_project || linked || self.confirm.is_some())
                            .label(tr(bardo, Text::SaveTemplate))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.save(false, window, cx)
                            })),
                    )
                    .child(
                        Button::new("revert-template")
                            .ghost()
                            .label(tr(bardo, Text::RevertTemplate))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.notice = None;
                                this.reload(window, cx)
                            })),
                    )
                    .child(
                        Button::new("default-template")
                            .outline()
                            .label(tr(bardo, Text::DefaultTemplate))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                let body = default_template(this.kind);
                                this.notice = None;
                                this.fill(
                                    body.instructions().to_owned(),
                                    body.prompt().to_owned(),
                                    window,
                                    cx,
                                );
                            })),
                    ),
                scroll,
            ))
            .children(confirm)
            .children(
                self.notice
                    .clone()
                    .map(|notice| kit::notice(Tone::Success, notice, cx)),
            )
            .children(
                self.error
                    .map(|error| kit::notice(Tone::Danger, tr(bardo, error), cx)),
            )
            .child(kit::anchor_in(
                TourAnchor::Control(Control::TemplateVariables),
                v_flex()
                    .pt_3()
                    .mt_1()
                    .gap_1p5()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_sm()
                                    .font_medium()
                                    .child(tr(bardo, Text::TemplateVariablesTitle)),
                            )
                            .child(guide::info(
                                bardo,
                                "template-variables-info",
                                tr(bardo, Text::TemplateVariablesHint),
                                guide::refs::TEMPLATES_VARIABLES,
                            )),
                    )
                    .children(variables),
                scroll,
            ))
            .children(shared)
    }
}

fn field(
    bardo: &Bardo,
    label: SharedString,
    hint: SharedString,
    input: AnyElement,
) -> gpui_kit::Div {
    let id = SharedString::from(format!("template-field-{label}"));
    let info = guide::info(bardo, id, hint, guide::refs::TEMPLATES_FIELDS);
    kit::field(label, Some(info), input, None)
}

impl Render for TemplatesScreen {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let collection = self.collection(cx);
        let editor = self.render_editor(cx).into_any_element();
        let bardo = self.bardo.read(cx);
        let mut header = Header::place(bardo, Destination::Templates, cx);
        let info = guide::info(
            bardo,
            "templates-info",
            tr(bardo, Text::TemplatesHint),
            guide::refs::TEMPLATES_VERSIONS,
        )
        .into_any_element();
        header.info = guide::header_info(
            bardo,
            Destination::Templates,
            self.has_content(),
            Some(info),
            cx,
        );
        let mut parts = ScreenParts::new(header);
        parts.collection = Some(collection);
        parts.inspector = Some(Inspector::new(vec![editor]));
        parts.scroll = Some(self.scroll.clone());
        layout::screen(parts, cx)
    }
}
