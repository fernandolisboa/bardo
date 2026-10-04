//! Templates screen: the prompts Bardo sends to the AI, one template per
//! kind (the script, the scene plan's image prompts). The versions are the
//! collection and the editor its inspector: it holds the current version's
//! text; picking an older version loads its text, and saving always creates
//! the next version.

use bardo_app::bardo_domain::{
    TemplateField, TemplateFieldError, TemplateKind, TemplateVersion, TemplateVersionId,
};
use std::rc::Rc;

use bardo_app::{Bardo, Control, Destination, Text, TourAnchor, default_template};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::{
    ActiveTheme as _, IconName, Selectable as _, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, ClickEvent, Entity, ScrollHandle, SharedString, Window, div, px};

use crate::appearance::look;
use crate::kit::{self, Tone};
use crate::parts::{Collection, CollectionKind, Header, Inspector, ScreenParts, Tile};
use crate::shell::tr;
use crate::{guide, layout};

pub struct TemplatesScreen {
    bardo: Entity<Bardo>,
    kind: TemplateKind,
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
}

impl TemplatesScreen {
    pub fn new(bardo: Entity<Bardo>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let instructions = cx.new(|cx| TextareaState::new(window, cx).auto_grow(4, 12));
        let prompt = cx.new(|cx| TextareaState::new(window, cx).auto_grow(6, 16));
        let mut screen = Self {
            bardo,
            kind: TemplateKind::Script,
            versions: Vec::new(),
            base: None,
            instructions,
            prompt,
            errors: Vec::new(),
            error: None,
            notice: None,
            scroll: ScrollHandle::new(),
        };
        screen.reload(window, cx);
        screen
    }

    /// Re-reads the versions and puts the current one in the editor.
    pub fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.bardo.read(cx).template_versions(self.kind) {
            Ok(versions) => {
                self.versions = versions;
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
        cx.notify();
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

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let instructions = self.instructions.read(cx).value();
        let prompt = self.prompt.read(cx).value();
        let current = self.versions.first().map(|version| version.id);
        let result = self
            .bardo
            .read(cx)
            .save_template(self.kind, &instructions, &prompt);
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
                            .label(tr(bardo, Text::SaveTemplate))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.save(window, cx)
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
        let mut header = Header::place(bardo, Destination::Templates);
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
