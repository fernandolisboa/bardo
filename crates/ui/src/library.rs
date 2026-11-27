//! What the Narrators, Products and Templates screens share as library
//! screens (#339): each shows the open project's items (with the global
//! ones it uses, tagged *Global*) on its project entry, or the global
//! library's on the Global library's tab. Editing a used global item lists
//! its uses before saving; *Bring into the project* copies a global item
//! into the open project under a name the user may change. Rules live in
//! `bardo_app`; this file holds the pieces the screens draw alike.

use bardo_app::bardo_domain::ProjectId;
use bardo_app::{Bardo, LibraryUse, Text};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{IconName, StyledExt as _, h_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, ClickEvent, ElementId, Entity, SharedString, Window, div};

use crate::appearance::look;
use crate::kit::{self, Tone};
use crate::shell::tr;

/// Whose items a library screen shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryScope {
    /// The open project's own, with the global ones it uses.
    Project,
    /// The global library's, on its tab.
    Global,
}

impl LibraryScope {
    /// The owner the screen lists and creates in (`None`: the global
    /// library), and whether a project scope has no project open.
    pub fn owner(self, bardo: &Bardo) -> (Option<ProjectId>, bool) {
        match self {
            LibraryScope::Global => (None, false),
            LibraryScope::Project => {
                let open = bardo
                    .open_project()
                    .ok()
                    .flatten()
                    .map(|project| project.id);
                (open, open.is_none())
            }
        }
    }
}

/// The open project's id and name, when one is open: where *Bring into
/// the project* copies to.
pub fn open_project(bardo: &Bardo) -> Option<(ProjectId, String)> {
    bardo
        .open_project()
        .ok()
        .flatten()
        .map(|project| (project.id, project.details.name().to_owned()))
}

/// One use as a line: `Project › Channel`, or `Project › Channel › Video`.
pub fn use_line(item: &LibraryUse) -> SharedString {
    let mut parts = vec![item.project.as_str(), item.channel.as_str()];
    parts.extend(item.video.as_deref());
    SharedString::from(format!("• {}", parts.join(" › ")))
}

/// The *Global* tag a project's list shows on a global item.
pub fn global_tag(bardo: &Bardo, cx: &App) -> AnyElement {
    kit::status_with(
        Tone::Info,
        IconName::Globe,
        tr(bardo, Text::LibraryGlobalTag),
        cx,
    )
    .into_any_element()
}

/// The note under a project item's title that a global item is shared by
/// reference: editing it changes it everywhere.
pub fn global_note(bardo: &Bardo, cx: &App) -> AnyElement {
    div()
        .text_sm()
        .text_color(look(cx).tokens.text2)
        .child(tr(bardo, Text::LibraryGlobalNote))
        .into_any_element()
}

/// The card that lists where a global item is used before a save goes
/// through, with *Save anyway* and *Cancel*.
pub fn confirm_uses<T: 'static>(
    id: &'static str,
    bardo: &Entity<Bardo>,
    (title, lines): (SharedString, Vec<SharedString>),
    on_confirm: impl Fn(&mut T, &mut Window, &mut Context<T>) + 'static,
    on_cancel: impl Fn(&mut T, &mut Window, &mut Context<T>) + 'static,
    cx: &mut Context<T>,
) -> AnyElement {
    let warning = look(cx).tokens.warning;
    let (save, cancel) = {
        let bardo = bardo.read(cx);
        (
            tr(bardo, Text::LibrarySaveAnyway),
            tr(bardo, Text::LibraryCancel),
        )
    };
    kit::card(cx)
        .p_4()
        .gap_2()
        .border_color(warning)
        .child(div().font_semibold().child(title))
        .children(lines.into_iter().map(|line| div().text_sm().child(line)))
        .child(
            h_flex()
                .gap_2()
                .pt_1()
                .child(
                    Button::new(ElementId::Name(format!("{id}-confirm").into()))
                        .warning()
                        .label(save)
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            on_confirm(this, window, cx)
                        })),
                )
                .child(
                    Button::new(ElementId::Name(format!("{id}-cancel").into()))
                        .ghost()
                        .label(cancel)
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            on_cancel(this, window, cx)
                        })),
                ),
        )
        .into_any_element()
}

/// *Bring into the project* in progress: the copy's name, and why the last
/// try was refused.
pub struct BringForm {
    pub name: Entity<InputState>,
    pub error: Option<Text>,
}

impl BringForm {
    /// A form starting from `name`.
    pub fn new<T: 'static>(name: &str, window: &mut Window, cx: &mut Context<T>) -> Self {
        let value = name.to_owned();
        let input = cx.new(|cx| InputState::new(window, cx).default_value(value));
        Self {
            name: input,
            error: None,
        }
    }

    /// The typed name, trimmed.
    pub fn typed(&self, cx: &App) -> String {
        self.name.read(cx).value().trim().to_owned()
    }
}

/// The *Bring into the project* card: what it does, the copy's name, and
/// *Bring it* / *Cancel*.
pub fn render_bring<T: 'static>(
    id: &'static str,
    bardo: &Entity<Bardo>,
    (form, project): (&BringForm, &str),
    on_confirm: impl Fn(&mut T, &mut Window, &mut Context<T>) + 'static,
    on_cancel: impl Fn(&mut T, &mut Window, &mut Context<T>) + 'static,
    cx: &mut Context<T>,
) -> AnyElement {
    let tokens = &look(cx).tokens;
    let (muted, danger) = (tokens.text2, tokens.danger);
    let (title, hint, name, error, confirm, cancel) = {
        let bardo = bardo.read(cx);
        (
            SharedString::from(bardo.text_with(Text::LibraryBringTitle, &[("project", project)])),
            tr(bardo, Text::LibraryBringHint),
            tr(bardo, Text::LibraryBringName),
            form.error.map(|error| tr(bardo, error)),
            tr(bardo, Text::LibraryBringConfirm),
            tr(bardo, Text::LibraryCancel),
        )
    };
    kit::card(cx)
        .p_4()
        .gap_2()
        .child(div().font_semibold().child(title))
        .child(div().text_sm().text_color(muted).child(hint))
        .child(kit::field(
            name,
            None,
            Input::new(&form.name).into_any_element(),
            error.map(|error| {
                div()
                    .text_xs()
                    .text_color(danger)
                    .child(error)
                    .into_any_element()
            }),
        ))
        .child(
            h_flex()
                .gap_2()
                .pt_1()
                .child(
                    Button::new(ElementId::Name(format!("{id}-confirm").into()))
                        .primary()
                        .label(confirm)
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            on_confirm(this, window, cx)
                        })),
                )
                .child(
                    Button::new(ElementId::Name(format!("{id}-cancel").into()))
                        .ghost()
                        .label(cancel)
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            on_cancel(this, window, cx)
                        })),
                ),
        )
        .into_any_element()
}

/// The *Bring into the project* button.
pub fn bring_button<T: 'static>(
    id: &'static str,
    bardo: &Entity<Bardo>,
    on_click: impl Fn(&mut T, &mut Window, &mut Context<T>) + 'static,
    cx: &mut Context<T>,
) -> AnyElement {
    let label = tr(bardo.read(cx), Text::LibraryBring);
    h_flex()
        .child(
            Button::new(id)
                .outline()
                .icon(IconName::Plus)
                .label(label)
                .on_click(
                    cx.listener(move |this, _: &ClickEvent, window, cx| on_click(this, window, cx)),
                ),
        )
        .into_any_element()
}
