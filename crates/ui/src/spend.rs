//! What a generation will cost, shown beside the button that starts it, and
//! the question asked when it would reach a provider's monthly budget.
//! The estimate and the budget rules live in `bardo_app`.

use bardo_app::bardo_domain::BudgetLevel;
use bardo_app::{Bardo, ProviderEstimate, SpendEstimate, Text};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, ClickEvent, ElementId, SharedString, Window, div};

use crate::shell::tr;

/// The estimate line, then one line per budget it takes past 80% or to
/// 100%. `cost` reads the amount (`Text::EstimateCost` or a variant with
/// an `{amount}` placeholder). Nothing when the estimate calls nobody.
pub(crate) fn estimate_note(
    bardo: &Bardo,
    estimate: &SpendEstimate,
    cost: Text,
    cx: &App,
) -> Option<AnyElement> {
    if estimate.providers.is_empty() {
        return None;
    }
    let theme = cx.theme();
    let amount = bardo.money(estimate.total());
    let line = if estimate.providers.iter().all(|p| p.amount.is_none()) {
        tr(bardo, Text::EstimateUnknown)
    } else if estimate.is_partial() {
        SharedString::from(bardo.text_with(Text::EstimatePartial, &[("amount", &amount)]))
    } else {
        SharedString::from(bardo.text_with(cost, &[("amount", &amount)]))
    };
    let near = estimate.near_budget().map(|provider| {
        div()
            .text_xs()
            .text_color(theme.warning)
            .child(near_line(bardo, provider))
    });
    let over = estimate.over_budget().map(|provider| {
        div()
            .text_xs()
            .text_color(theme.danger)
            .child(over_line(bardo, provider))
    });
    Some(
        v_flex()
            .gap_0p5()
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(line),
            )
            .children(near)
            .children(over)
            .into_any_element(),
    )
}

fn near_line(bardo: &Bardo, provider: &ProviderEstimate) -> SharedString {
    SharedString::from(bardo.text_with(
        Text::EstimateNear,
        &[
            (
                "provider",
                &bardo.text(Text::ProviderName(provider.provider)),
            ),
            ("spent", &bardo.money(provider.spent)),
            ("budget", &bardo.money(provider.budget.unwrap_or_default())),
        ],
    ))
}

fn over_line(bardo: &Bardo, provider: &ProviderEstimate) -> SharedString {
    SharedString::from(bardo.text_with(
        Text::BudgetReachedLine,
        &[
            (
                "provider",
                &bardo.text(Text::ProviderName(provider.provider)),
            ),
            ("spent", &bardo.money(provider.spent)),
            ("budget", &bardo.money(provider.budget.unwrap_or_default())),
            ("amount", &bardo.money(provider.amount.unwrap_or_default())),
        ],
    ))
}

/// Asks before starting a job that would reach a budget: the budgets it
/// reaches, then "generate anyway" and "cancel".
pub(crate) fn budget_question(
    id: impl Into<SharedString>,
    bardo: &Bardo,
    estimate: &SpendEstimate,
    cx: &App,
    confirm: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cancel: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let id: SharedString = id.into();
    let theme = cx.theme();
    let lines = estimate
        .providers
        .iter()
        .filter(|provider| provider.level == Some(BudgetLevel::Reached))
        .map(|provider| div().text_sm().child(over_line(bardo, provider)));
    v_flex()
        .p_3()
        .gap_2()
        .rounded_md()
        .border_1()
        .border_color(theme.danger)
        .child(
            div()
                .font_semibold()
                .text_color(theme.danger)
                .child(tr(bardo, Text::BudgetReachedTitle)),
        )
        .children(lines)
        .child(div().text_sm().child(tr(bardo, Text::BudgetQuestion)))
        .child(
            h_flex()
                .gap_2()
                .child(
                    Button::new(ElementId::Name(format!("{id}-confirm").into()))
                        .danger()
                        .small()
                        .label(tr(bardo, Text::BudgetConfirm))
                        .on_click(confirm),
                )
                .child(
                    Button::new(ElementId::Name(format!("{id}-cancel").into()))
                        .ghost()
                        .small()
                        .label(tr(bardo, Text::BudgetCancel))
                        .on_click(cancel),
                ),
        )
        .into_any_element()
}
