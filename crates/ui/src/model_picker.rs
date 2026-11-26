//! A video model picked one level at a time (#329): provider, model,
//! version, then variant, each its own short menu. Every place that picks
//! a clip or take model uses it, so they look and behave the same.

use std::rc::Rc;

use bardo_app::bardo_domain::{
    ClipModelRef, LevelChoices, LevelPick, ModelLevel, ModelName, ModelOption, model_levels,
    pick_model,
};
use bardo_app::{Bardo, Text};
use gpui_kit::component::button::Button;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, ElementId, SharedString, Window, div};

use crate::shell::tr;

/// What picking a model does: `None` follows the default.
pub type OnModel = Rc<dyn Fn(Option<ClipModelRef>, &mut Window, &mut App)>;

/// Following a default: the text that names it, with `{model}` for the
/// model ("{model} (default)"), and the model it is today.
pub struct Follow {
    pub text: Text,
    pub default: Option<ClipModelRef>,
}

/// A model picker's state for one render.
pub struct ModelPicker {
    /// Unique on the screen; each level's button adds its own suffix.
    pub id: SharedString,
    /// The models offered, in the provider's order (its default first).
    pub models: Vec<(ClipModelRef, ModelName)>,
    /// The model picked; `None` follows the default.
    pub value: Option<ClipModelRef>,
    /// The choice that follows the default; `None` when the picker has no
    /// default to follow.
    pub follow: Option<Follow>,
    pub disabled: bool,
    /// Smaller buttons, for a row in a list.
    pub compact: bool,
    pub on_pick: OnModel,
}

impl ModelPicker {
    pub fn new(
        id: impl Into<SharedString>,
        models: Vec<(ClipModelRef, ModelName)>,
        value: Option<ClipModelRef>,
        on_pick: OnModel,
    ) -> Self {
        Self {
            id: id.into(),
            models,
            value,
            follow: None,
            disabled: false,
            compact: false,
            on_pick,
        }
    }

    /// Offers following `default`, named by `text`.
    pub fn follow(mut self, text: Text, default: Option<ClipModelRef>) -> Self {
        self.follow = Some(Follow { text, default });
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }

    /// The levels, each a caption over a menu button, in one wrapping row.
    /// Following the default shows the default's levels, the provider's
    /// named as the default; picking any level picks a model of its own.
    pub fn render(self, bardo: &Bardo, cx: &App) -> AnyElement {
        // Nothing offered and nothing to follow: nothing to pick (the place
        // says why).
        if self.models.is_empty() && self.follow.is_none() {
            return div().into_any_element();
        }
        let options: Vec<ModelOption<'_>> = self
            .models
            .iter()
            .map(|(id, name)| ModelOption { id, name })
            .collect();
        let shown = self.shown();
        let levels = shown.and_then(|shown| model_levels(&options, shown));
        let current = shown.and_then(|shown| options.iter().find(|option| option.id == shown));

        let mut row = h_flex().flex_wrap().gap_x_2().gap_y_1().items_end();
        match (&levels, current) {
            (Some(levels), Some(current)) => {
                for choices in levels {
                    row = row.child(self.level(bardo, cx, &options, choices, current));
                }
            }
            _ => {
                // A model no longer offered, or no default to show: only
                // the provider menu, named after what is used now.
                let label = match &self.value {
                    Some(gone) => SharedString::from(gone.model().to_owned()),
                    None => self.follow_label(bardo, None),
                };
                let mut providers = Vec::new();
                for option in &options {
                    if !providers.contains(&option.id.provider()) {
                        providers.push(option.id.provider());
                    }
                }
                let choices = LevelChoices {
                    level: ModelLevel::Provider,
                    providers,
                    parts: Vec::new(),
                };
                row = row.child(self.menu(bardo, cx, &options, &choices, label, None));
            }
        }
        row.into_any_element()
    }

    /// The model whose levels show: the one picked, else the default.
    fn shown(&self) -> Option<&ClipModelRef> {
        self.value.as_ref().or_else(|| {
            self.follow
                .as_ref()
                .and_then(|follow| follow.default.as_ref())
        })
    }

    /// The follow text with `model` in it (`None`: the default's full
    /// title).
    fn follow_label(&self, bardo: &Bardo, model: Option<&str>) -> SharedString {
        let Some(follow) = &self.follow else {
            return SharedString::default();
        };
        let title = match model {
            Some(model) => model.to_owned(),
            None => follow
                .default
                .as_ref()
                .and_then(|default| {
                    self.models
                        .iter()
                        .find(|(id, _)| id == default)
                        .map(|(id, name)| titled(bardo, name, id))
                })
                .unwrap_or_else(|| "—".to_owned()),
        };
        SharedString::from(bardo.text_with(follow.text, &[("model", &title)]))
    }

    /// One level: its caption, then a button naming the current choice
    /// that opens its choices.
    fn level(
        &self,
        bardo: &Bardo,
        cx: &App,
        options: &[ModelOption<'_>],
        choices: &LevelChoices,
        current: &ModelOption<'_>,
    ) -> AnyElement {
        let label = match choices.level {
            ModelLevel::Provider if self.value.is_none() => {
                let provider = bardo.text(Text::ProviderName(current.id.provider()));
                self.follow_label(bardo, Some(&provider))
            }
            ModelLevel::Provider => tr(bardo, Text::ProviderName(current.id.provider())),
            level => part_label(current.name.part(level)),
        };
        self.menu(bardo, cx, options, choices, label, Some(current))
    }

    fn menu(
        &self,
        bardo: &Bardo,
        cx: &App,
        options: &[ModelOption<'_>],
        choices: &LevelChoices,
        label: SharedString,
        current: Option<&ModelOption<'_>>,
    ) -> AnyElement {
        let level = choices.level;
        let from = self.shown().cloned();
        // Each choice with the model it leads to.
        let mut items: Vec<(SharedString, bool, Option<ClipModelRef>)> = Vec::new();
        match level {
            ModelLevel::Provider => {
                for provider in &choices.providers {
                    let to =
                        pick_model(options, from.as_ref(), LevelPick::Provider(*provider)).cloned();
                    let checked = self.value.is_some()
                        && current.is_some_and(|current| current.id.provider() == *provider);
                    items.push((tr(bardo, Text::ProviderName(*provider)), checked, to));
                }
            }
            level => {
                for part in &choices.parts {
                    let to =
                        pick_model(options, from.as_ref(), LevelPick::Part(level, part)).cloned();
                    let checked = current.is_some_and(|current| current.name.part(level) == part);
                    items.push((part_label(part), checked, to));
                }
            }
        }
        let follow = (level == ModelLevel::Provider && self.follow.is_some())
            .then(|| self.follow_label(bardo, None));
        let following = self.value.is_none();
        let default = self
            .follow
            .as_ref()
            .and_then(|follow| follow.default.clone());
        // A level with one choice and nothing else to offer says what it
        // is without opening.
        let single = items.len() <= 1 && follow.is_none();
        let on_pick = Rc::clone(&self.on_pick);
        let mut button = Button::new(ElementId::Name(
            format!("{}-{}", self.id, level.code()).into(),
        ))
        .outline()
        .label(label)
        .disabled(self.disabled || single);
        button = if self.compact {
            button.xsmall()
        } else {
            button.small()
        };
        let control: AnyElement = if single {
            button.into_any_element()
        } else {
            button
                .dropdown_caret(true)
                .dropdown_menu(move |menu, _, _| {
                    let mut menu = menu.scrollable(true);
                    if let Some(follow) = &follow {
                        let on_pick = Rc::clone(&on_pick);
                        menu = menu
                            .item(
                                PopupMenuItem::new(follow.clone())
                                    .checked(following)
                                    .on_click(move |_, window, cx| on_pick(None, window, cx)),
                            )
                            .separator();
                    }
                    for (title, checked, to) in &items {
                        let on_pick = Rc::clone(&on_pick);
                        let to = to.clone();
                        let default = default.clone();
                        menu = menu.item(
                            PopupMenuItem::new(title.clone())
                                .checked(*checked)
                                .disabled(to.is_none())
                                .on_click(move |_, window, cx| {
                                    // While following, the default itself
                                    // keeps following it.
                                    match to.clone() {
                                        Some(to) if following && default.as_ref() == Some(&to) => {
                                            on_pick(None, window, cx)
                                        }
                                        Some(to) => on_pick(Some(to), window, cx),
                                        None => {}
                                    }
                                }),
                        );
                    }
                    menu
                })
                .into_any_element()
        };
        v_flex()
            .gap_0p5()
            .min_w_0()
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr(bardo, Text::ModelLevelName(level))),
            )
            .child(control)
            .into_any_element()
    }
}

/// A part as a menu shows it; an empty one (a version with and without a
/// variant) as a dash.
fn part_label(part: &str) -> SharedString {
    if part.is_empty() {
        SharedString::from("—")
    } else {
        SharedString::from(part.to_owned())
    }
}

/// `model` by name and provider, as the default choice names it.
pub fn titled(bardo: &Bardo, name: &ModelName, model: &ClipModelRef) -> String {
    format!(
        "{name} · {}",
        bardo.text(Text::ProviderName(model.provider()))
    )
}
