//! Costs screen: a month at a glance (spent, budgets in alert, calls
//! without a price), each paid provider's spend against its monthly budget,
//! the rate table that prices calls, and the spend by channel and video. Spend, budgets and rates live in `bardo_app`; this file
//! maps clicks to use cases and results to text.

use std::rc::Rc;
use std::time::Duration;

use bardo_app::bardo_domain::{BudgetLevel, Meter, Month, Provider};
use bardo_app::{
    Bardo, Control, CostsView, Destination, ProviderSpend, RateRow, SpendRow, Text, TourAnchor,
};
use gpui_kit::component::button::{Button, ButtonGroup, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::{
    ActiveTheme as _, IconName, Selectable as _, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Entity, ScrollHandle, SharedString, Subscription, Task, Window,
    div, px, relative,
};

use crate::appearance::look;
use crate::guide;
use crate::icons::Lucide;
use crate::kit::{self, Tone};
use crate::layout;
use crate::parts::{Figure, Header, ScreenParts, Section, Sections};
use crate::shell::tr;

/// How often the screen checks the job queue for new costs.
const POLL_EVERY: Duration = Duration::from_millis(250);

/// The rate whose price is open in the rate editor.
#[derive(Clone, PartialEq, Eq)]
struct RateKey {
    provider: Provider,
    model: String,
    meter: Meter,
}

impl RateKey {
    fn of(row: &RateRow) -> Self {
        Self {
            provider: row.rate.provider,
            model: row.rate.model.clone(),
            meter: row.rate.meter,
        }
    }
}

/// The costs screen's tabs.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CostsTab {
    Budgets,
    Rates,
}

pub struct CostsScreen {
    bardo: Entity<Bardo>,
    month: Month,
    tab: CostsTab,
    view: Option<CostsView>,
    /// Why the last change did not happen.
    error: Option<Text>,
    notice: Option<Text>,
    /// The provider whose budget is open in `budget_input`.
    editing_budget: Option<Provider>,
    budget_input: Entity<InputState>,
    budget_error: Option<Text>,
    editing_rate: Option<RateKey>,
    rate_input: Entity<InputState>,
    rate_error: Option<Text>,
    new_provider: Provider,
    new_meter: Meter,
    new_model: Entity<InputState>,
    new_price: Entity<InputState>,
    add_error: Option<Text>,
    revision: u64,
    /// The page's scroll, so a tour brings what it lights into view.
    scroll: ScrollHandle,
    _poll: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl CostsScreen {
    pub fn new(bardo: Entity<Bardo>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let budget_input = cx.new(|cx| InputState::new(window, cx));
        let rate_input = cx.new(|cx| InputState::new(window, cx));
        let new_model = cx.new(|cx| InputState::new(window, cx));
        let new_price = cx.new(|cx| InputState::new(window, cx));
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL_EVERY).await;
                if this.update(cx, |this, cx| this.poll(cx)).is_err() {
                    break;
                }
            }
        });
        let mut subscriptions = vec![cx.observe_in(&bardo, window, |this, _, window, cx| {
            this.relabel(window, cx)
        })];
        for input in [&budget_input, &rate_input, &new_model, &new_price] {
            subscriptions.push(cx.subscribe_in(input, window, Self::on_input));
        }
        let month = bardo.read(cx).current_month();
        let revision = bardo.read(cx).jobs_revision();
        let mut screen = Self {
            bardo,
            month,
            tab: CostsTab::Budgets,
            view: None,
            error: None,
            notice: None,
            editing_budget: None,
            budget_input,
            budget_error: None,
            editing_rate: None,
            rate_input,
            rate_error: None,
            new_provider: Provider::Claude,
            new_meter: Meter::InputTokens,
            new_model,
            new_price,
            add_error: None,
            revision,
            scroll: ScrollHandle::new(),
            _poll: poll,
            _subscriptions: subscriptions,
        };
        screen.relabel(window, cx);
        screen.load(cx);
        screen
    }

    /// Typing clears the last error; Enter saves what is open.
    fn on_input(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => {
                self.budget_error = None;
                self.rate_error = None;
                self.add_error = None;
                cx.notify();
            }
            InputEvent::PressEnter { .. } => {
                if *input == self.budget_input {
                    self.save_budget(cx);
                } else if *input == self.rate_input {
                    self.save_rate(cx);
                } else {
                    self.add_rate(window, cx);
                }
            }
            _ => {}
        }
    }

    /// Re-reads the month's spend (jobs ran elsewhere meanwhile).
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.notice = None;
        self.error = None;
        self.load(cx);
        cx.notify();
    }

    /// Placeholders follow the interface language.
    fn relabel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for (input, text) in [
            (&self.budget_input, Text::BudgetPlaceholder),
            (&self.rate_input, Text::RatePricePlaceholder),
            (&self.new_model, Text::RateModelPlaceholder),
            (&self.new_price, Text::RatePricePlaceholder),
        ] {
            let placeholder = tr(self.bardo.read(cx), text);
            input.update(cx, |input, cx| {
                input.set_placeholder(placeholder, window, cx)
            });
        }
        cx.notify();
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        match self.bardo.read(cx).costs(self.month) {
            Ok(view) => self.view = Some(view),
            Err(_) => {
                self.view = None;
                self.error = Some(Text::CostsNotLoaded);
            }
        }
    }

    fn poll(&mut self, cx: &mut Context<Self>) {
        let revision = self.bardo.read(cx).jobs_revision();
        if revision != self.revision {
            self.revision = revision;
            self.load(cx);
            cx.notify();
        }
    }

    fn show_month(&mut self, month: Month, cx: &mut Context<Self>) {
        self.month = month;
        self.load(cx);
        cx.notify();
    }

    /// Shows the outcome of a change and re-reads the screen.
    fn done(&mut self, notice: Text, cx: &mut Context<Self>) {
        self.notice = Some(notice);
        self.error = None;
        self.load(cx);
        cx.notify();
    }

    fn edit_budget(&mut self, spend: &ProviderSpend, window: &mut Window, cx: &mut Context<Self>) {
        let typed = spend.budget.map(|b| b.to_string()).unwrap_or_default();
        self.budget_input.update(cx, |input, cx| {
            input.set_value(typed, window, cx);
            input.focus(window, cx);
        });
        self.editing_budget = Some(spend.provider);
        self.budget_error = None;
        self.notice = None;
        cx.notify();
    }

    fn save_budget(&mut self, cx: &mut Context<Self>) {
        let Some(provider) = self.editing_budget else {
            return;
        };
        let typed = self.budget_input.read(cx).value();
        match self.bardo.read(cx).set_budget(provider, &typed) {
            Ok(_) => {
                self.editing_budget = None;
                self.budget_error = None;
                self.done(Text::BudgetSaved, cx);
            }
            Err(error) => {
                self.budget_error = Some(error.message());
                cx.notify();
            }
        }
    }

    fn remove_budget(&mut self, provider: Provider, cx: &mut Context<Self>) {
        self.editing_budget = None;
        match self.bardo.read(cx).remove_budget(provider) {
            Ok(()) => self.done(Text::BudgetRemoved, cx),
            Err(error) => {
                self.error = Some(error.message());
                cx.notify();
            }
        }
    }

    fn edit_rate(&mut self, row: &RateRow, window: &mut Window, cx: &mut Context<Self>) {
        let typed = row.rate.price.to_string();
        self.rate_input.update(cx, |input, cx| {
            input.set_value(typed, window, cx);
            input.focus(window, cx);
        });
        self.editing_rate = Some(RateKey::of(row));
        self.rate_error = None;
        self.notice = None;
        cx.notify();
    }

    fn save_rate(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.editing_rate.clone() else {
            return;
        };
        let typed = self.rate_input.read(cx).value();
        match self
            .bardo
            .read(cx)
            .save_rate(key.provider, &key.model, key.meter, &typed)
        {
            Ok(_) => {
                self.editing_rate = None;
                self.done(Text::RateSaved, cx);
            }
            Err(error) => {
                self.rate_error = Some(error.message());
                cx.notify();
            }
        }
    }

    fn reset_rate(&mut self, key: RateKey, cx: &mut Context<Self>) {
        self.editing_rate = None;
        match self
            .bardo
            .read(cx)
            .reset_rate(key.provider, &key.model, key.meter)
        {
            Ok(()) => self.done(Text::RateSaved, cx),
            Err(error) => {
                self.error = Some(error.message());
                cx.notify();
            }
        }
    }

    fn add_rate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let model = self.new_model.read(cx).value();
        let price = self.new_price.read(cx).value();
        match self
            .bardo
            .read(cx)
            .save_rate(self.new_provider, model.trim(), self.new_meter, &price)
        {
            Ok(_) => {
                self.add_error = None;
                for input in [&self.new_model, &self.new_price] {
                    input.update(cx, |input, cx| input.set_value("", window, cx));
                }
                self.done(Text::RateSaved, cx);
            }
            Err(error) => {
                self.add_error = Some(error.message());
                cx.notify();
            }
        }
    }

    /// Previous month, the month, next month (up to this one).
    fn month_switch(&self, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let current = bardo.current_month();
        let month = self.month;
        let switch = h_flex()
            .gap_1()
            .items_center()
            .child(
                Button::new("previous-month")
                    .ghost()
                    .small()
                    .icon(IconName::ChevronLeft)
                    .tooltip(tr(bardo, Text::CostsPreviousMonth))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.show_month(month.previous(), cx)
                    })),
            )
            .child(
                div()
                    .min_w(px(150.))
                    .text_center()
                    .font_semibold()
                    .child(SharedString::from(bardo.month_name(month))),
            )
            .child(div().w(px(28.)).when(month < current, |slot| {
                slot.child(
                    Button::new("next-month")
                        .ghost()
                        .small()
                        .icon(IconName::ChevronRight)
                        .tooltip(tr(bardo, Text::CostsNextMonth))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.show_month(month.next(), cx)
                        })),
                )
            }));
        kit::anchor(TourAnchor::Control(Control::CostsMonth), switch).into_any_element()
    }

    /// The month in three figures: what was spent, the budgets in alert and
    /// the calls without a price.
    fn figures(&self, cx: &mut Context<Self>) -> Vec<Figure> {
        let Some(view) = self.view.as_ref() else {
            return Vec::new();
        };
        let summary = view.summary();
        let unpriced = view.unpriced.first().cloned();
        let add_price = unpriced.clone().map(|(provider, model)| {
            Button::new("add-price")
                .link()
                .xsmall()
                .label(tr(self.bardo.read(cx), Text::AddPrice))
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.add_price(provider, &model, window, cx)
                }))
        });
        let bardo = self.bardo.read(cx);
        let t = look(cx).tokens;
        let mono = cx.theme().mono_font_family.clone();
        let small = |text: SharedString| {
            div()
                .text_xs()
                .text_color(t.text2)
                .child(text)
                .into_any_element()
        };
        let chip = |tone: Tone, text: String| {
            kit::status(tone, SharedString::from(text), cx).into_any_element()
        };

        let mut spent = Figure::new(
            bardo.text_with(
                Text::CostsTotal,
                &[("month", &bardo.month_name(view.month))],
            ),
            bardo.money(summary.total),
        );
        spent.line = Some(small(SharedString::from(bardo.text_with(
            Text::CostsAcrossProviders,
            &[("n", &summary.providers_used.to_string())],
        ))));
        spent.brief = Some(
            h_flex()
                .gap_1()
                .items_baseline()
                .text_sm()
                .child(
                    div()
                        .font_semibold()
                        .font_family(mono.clone())
                        .child(SharedString::from(bardo.money(summary.total))),
                )
                .child(
                    div()
                        .text_color(t.text2)
                        .child(tr(bardo, Text::CostsBriefSpent)),
                )
                .into_any_element(),
        );

        let mut budgets = Figure::new(
            tr(bardo, Text::CostsBudgetsTile),
            summary.alerts.len().to_string(),
        );
        budgets.beside = (summary.budgets > 0).then(|| {
            SharedString::from(bardo.text_with(
                Text::CostsBudgetsInAlert,
                &[("total", &summary.budgets.to_string())],
            ))
        });
        budgets.line = Some(if summary.budgets == 0 {
            small(tr(bardo, Text::CostsNoBudgets))
        } else {
            h_flex()
                .gap_1()
                .flex_wrap()
                .children(summary.alerts.iter().map(|(provider, level)| {
                    let tone = if *level == BudgetLevel::Reached {
                        Tone::Danger
                    } else {
                        Tone::Warning
                    };
                    kit::status(tone, tr(bardo, Text::ProviderName(*provider)), cx)
                }))
                .into_any_element()
        });
        let over = summary
            .alerts
            .iter()
            .filter(|(_, level)| *level == BudgetLevel::Reached)
            .count();
        let near = summary.alerts.len() - over;
        budgets.brief = Some(
            h_flex()
                .gap_1()
                .children((over > 0).then(|| {
                    chip(
                        Tone::Danger,
                        bardo.text_with(Text::CostsBriefOver, &[("n", &over.to_string())]),
                    )
                }))
                .children((near > 0).then(|| {
                    chip(
                        Tone::Warning,
                        bardo.text_with(Text::CostsBriefNear, &[("n", &near.to_string())]),
                    )
                }))
                .into_any_element(),
        );

        let count = view.unpriced.len();
        let mut unpriced_figure =
            Figure::new(tr(bardo, Text::CostsUnpricedTitle), count.to_string());
        unpriced_figure.beside = (count > 0).then(|| {
            tr(
                bardo,
                if count == 1 {
                    Text::CostsUnpricedModel
                } else {
                    Text::CostsUnpricedModels
                },
            )
        });
        unpriced_figure.tone = (count > 0).then_some(Tone::Warning);
        unpriced_figure.line = Some(match unpriced {
            Some((_, model)) => {
                let models = view
                    .unpriced
                    .iter()
                    .map(|(provider, model)| {
                        format!("{} {model}", bardo.text(Text::ProviderName(*provider)))
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                h_flex()
                    .gap_1()
                    .items_center()
                    .text_xs()
                    .child(
                        div()
                            .font_family(mono.clone())
                            .text_color(t.warning)
                            .child(SharedString::from(model)),
                    )
                    .children(add_price)
                    .child(guide::info(
                        bardo,
                        "unpriced-info",
                        SharedString::from(
                            bardo.text_with(Text::CostsUnpriced, &[("models", &models)]),
                        ),
                        guide::refs::COSTS_RATES,
                    ))
                    .into_any_element()
            }
            None => small(tr(bardo, Text::CostsUnpricedNone)),
        });
        unpriced_figure.brief = Some(
            div()
                .children((count > 0).then(|| {
                    chip(
                        Tone::Warning,
                        bardo.text_with(Text::CostsBriefUnpriced, &[("n", &count.to_string())]),
                    )
                }))
                .into_any_element(),
        );

        vec![spent, budgets, unpriced_figure]
    }

    /// Opens the rate table with the model that has no price filled in.
    fn add_price(
        &mut self,
        provider: Provider,
        model: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.tab = CostsTab::Rates;
        self.new_provider = provider;
        self.add_error = None;
        let model = model.to_owned();
        self.new_model.update(cx, |input, cx| {
            input.set_value(model, window, cx);
        });
        self.new_price
            .update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    /// Every paid provider's month against its budget, as a table.
    fn budgets_table(&self, cx: &mut Context<Self>) -> AnyElement {
        let rows: Vec<AnyElement> = self
            .view
            .iter()
            .flat_map(|view| view.providers.iter())
            .map(|spend| self.budget_row(spend, cx))
            .collect();
        let bardo = self.bardo.read(cx);
        let t = look(cx).tokens;
        let heading = |text: Text| tr(bardo, text);
        let table = kit::card(cx)
            .overflow_hidden()
            .child(
                columns(
                    div().child(heading(Text::CostsColumnProvider)),
                    div().child(heading(Text::CostsColumnUsage)),
                    div().child(heading(Text::CostsColumnSpent)),
                    div().child(heading(Text::CostsColumnBudget)),
                    div().child(heading(Text::CostsColumnState)),
                    h_flex().child(guide::info(
                        bardo,
                        "costs-providers-info",
                        tr(bardo, Text::CostsProvidersHint),
                        guide::refs::COSTS_BUDGETS,
                    )),
                )
                .py_2()
                .text_xs()
                .font_semibold()
                .text_color(t.text2),
            )
            .children(rows);
        kit::anchor_in(
            TourAnchor::Control(Control::CostsBudgets),
            table,
            Some(&self.scroll),
        )
        .into_any_element()
    }

    fn budget_row(&self, spend: &ProviderSpend, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let t = look(cx).tokens;
        let mono = theme.mono_font_family.clone();
        let provider = spend.provider;
        let editing = self.editing_budget == Some(provider);
        let level_color = match spend.level {
            Some(BudgetLevel::Reached) => theme.danger,
            Some(BudgetLevel::Warning) => theme.warning,
            _ => t.accent,
        };

        let usage: AnyElement = if editing {
            v_flex()
                .gap_1()
                .child(
                    h_flex()
                        .gap_2()
                        .child(div().flex_1().child(Input::new(&self.budget_input).small()))
                        .child(
                            Button::new(("save-budget", provider as usize))
                                .primary()
                                .small()
                                .label(tr(bardo, Text::SaveBudget))
                                .on_click(
                                    cx.listener(|this, _: &ClickEvent, _, cx| this.save_budget(cx)),
                                ),
                        )
                        .child(
                            Button::new(("cancel-budget", provider as usize))
                                .ghost()
                                .small()
                                .label(tr(bardo, Text::CancelBudget))
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.editing_budget = None;
                                    this.budget_error = None;
                                    cx.notify();
                                })),
                        ),
                )
                .children(
                    self.budget_error
                        .map(|error| kit::notice(Tone::Danger, tr(bardo, error), cx).text_xs()),
                )
                .into_any_element()
        } else {
            match spend.percent {
                Some(percent) => h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div().flex_1().child(
                            Progress::new(("budget-progress", provider as usize))
                                .small()
                                .color(level_color)
                                .value(percent.min(100) as f32),
                        ),
                    )
                    .child(
                        div()
                            .w(px(40.))
                            .text_right()
                            .text_xs()
                            .text_color(t.text2)
                            .font_family(mono.clone())
                            .child(SharedString::from(format!("{percent}%"))),
                    )
                    .into_any_element(),
                None => div()
                    .text_sm()
                    .text_color(t.text2)
                    .child(tr(bardo, Text::BudgetNone))
                    .into_any_element(),
            }
        };

        let state = match spend.level {
            Some(BudgetLevel::Reached) => Some(kit::status(
                Tone::Danger,
                tr(bardo, Text::BudgetReached),
                cx,
            )),
            Some(BudgetLevel::Warning) => {
                Some(kit::status(Tone::Warning, tr(bardo, Text::BudgetNear), cx))
            }
            _ => None,
        };

        let row = spend.clone();
        let actions = (!editing).then(|| match spend.budget {
            Some(_) => h_flex()
                .gap_1()
                .child(
                    Button::new(("change-budget", provider as usize))
                        .ghost()
                        .xsmall()
                        .icon(Lucide::Pencil)
                        .tooltip(tr(bardo, Text::ChangeBudget))
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.edit_budget(&row, window, cx)
                        })),
                )
                .child(
                    Button::new(("remove-budget", provider as usize))
                        .ghost()
                        .xsmall()
                        .icon(Lucide::Trash)
                        .tooltip(tr(bardo, Text::RemoveBudget))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.remove_budget(provider, cx)
                        })),
                ),
            None => h_flex().child(
                Button::new(("set-budget", provider as usize))
                    .outline()
                    .xsmall()
                    .label(tr(bardo, Text::SetBudget))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.edit_budget(&row, window, cx)
                    })),
            ),
        });

        columns(
            div()
                .font_semibold()
                .child(tr(bardo, Text::ProviderName(provider))),
            usage,
            div()
                .font_family(mono.clone())
                .child(SharedString::from(bardo.money(spend.spent))),
            div()
                .font_family(mono)
                .text_color(t.text2)
                .child(SharedString::from(
                    spend
                        .budget
                        .map_or_else(|| "—".to_owned(), |b| bardo.money(b)),
                )),
            h_flex().children(state),
            h_flex().children(actions),
        )
        .py_2p5()
        .text_sm()
        .border_t(t.border_width)
        .border_color(t.border)
        .into_any_element()
    }

    /// Spend by channel (with bars) and the videos that cost the most.
    fn breakdown(&self, cx: &App) -> Vec<AnyElement> {
        let Some(view) = self.view.as_ref() else {
            return Vec::new();
        };
        let bardo = self.bardo.read(cx);
        let t = look(cx).tokens;
        if view.channels.is_empty() && view.videos.is_empty() {
            return vec![
                kit::card(cx)
                    .p_4()
                    .child(hint(
                        cx,
                        SharedString::from(bardo.text_with(
                            Text::CostsEmpty,
                            &[("month", &bardo.month_name(view.month))],
                        )),
                    ))
                    .into_any_element(),
            ];
        }
        let most = view
            .channels
            .iter()
            .map(|row| row.amount)
            .max()
            .unwrap_or_default();
        let channels = view.channels.iter().map(|row| {
            let share = if most.is_zero() {
                0.
            } else {
                row.amount.micros() as f32 / most.micros() as f32
            };
            v_flex()
                .gap_1()
                .py_1()
                .child(spend_line(
                    cx,
                    row.name
                        .clone()
                        .map_or_else(|| tr(bardo, Text::CostsUnknownChannel), SharedString::from),
                    None,
                    bardo.money(row.amount),
                ))
                .child(
                    div()
                        .h(px(4.))
                        .w_full()
                        .rounded(t.radius)
                        .bg(t.sunken)
                        .child(
                            div()
                                .h_full()
                                .rounded(t.radius)
                                .bg(t.text2)
                                .w(relative(share)),
                        ),
                )
        });
        let videos = view.videos.iter().map(|row: &SpendRow| {
            div()
                .py_1()
                .border_t(t.border_width)
                .border_color(t.border)
                .child(spend_line(
                    cx,
                    row.name
                        .clone()
                        .map_or_else(|| tr(bardo, Text::CostsUnknownVideo), SharedString::from),
                    row.channel.clone().map(SharedString::from),
                    bardo.money(row.amount),
                ))
        });
        // The aside scrolls apart from the page in Studio, so the tour does
        // not scroll to these: they lead it.
        vec![
            kit::anchor(
                TourAnchor::Control(Control::CostsChannels),
                kit::card(cx)
                    .p_3()
                    .gap_1()
                    .child(section_title(cx, tr(bardo, Text::CostsChannelsTitle)))
                    .children(channels),
            )
            .into_any_element(),
            kit::anchor(
                TourAnchor::Control(Control::CostsVideos),
                kit::card(cx)
                    .p_3()
                    .gap_1()
                    .child(section_title(cx, tr(bardo, Text::CostsVideosTitle)))
                    .children(videos),
            )
            .into_any_element(),
        ]
    }

    fn render_rates(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let rows: Vec<AnyElement> = self
            .view
            .iter()
            .flat_map(|view| view.rates.iter())
            .enumerate()
            .map(|(ix, row)| self.render_rate(ix, row, cx))
            .collect();
        let bardo = self.bardo.read(cx);
        let rates = v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_1()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(guide::info(
                        bardo,
                        "rates-info",
                        tr(bardo, Text::RatesHint),
                        guide::refs::COSTS_RATES,
                    )),
            )
            .child(kit::card(cx).children(rows))
            .child(self.render_add_rate(cx));
        kit::anchor_in(
            TourAnchor::Control(Control::CostsRates),
            rates,
            Some(&self.scroll),
        )
    }

    fn render_rate(&self, ix: usize, row: &RateRow, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let key = RateKey::of(row);
        let editing = self.editing_rate.as_ref() == Some(&key);
        let rate = &row.rate;
        let model = if rate.model.is_empty() {
            tr(bardo, Text::RateAllModels)
        } else {
            SharedString::from(rate.model.clone())
        };
        let tag = match (row.changed, row.default) {
            (true, Some(_)) => Some(kit::status(Tone::Info, tr(bardo, Text::RateChanged), cx)),
            (true, None) => Some(kit::status(Tone::Neutral, tr(bardo, Text::RateAdded), cx)),
            _ => None,
        };

        let price: AnyElement = if editing {
            v_flex()
                .gap_1()
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            div()
                                .w(px(120.))
                                .child(Input::new(&self.rate_input).small()),
                        )
                        .child(
                            Button::new(("save-rate", ix))
                                .primary()
                                .xsmall()
                                .label(tr(bardo, Text::SaveRate))
                                .on_click(
                                    cx.listener(|this, _: &ClickEvent, _, cx| this.save_rate(cx)),
                                ),
                        )
                        .child(
                            Button::new(("cancel-rate", ix))
                                .ghost()
                                .xsmall()
                                .label(tr(bardo, Text::CancelRate))
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.editing_rate = None;
                                    this.rate_error = None;
                                    cx.notify();
                                })),
                        ),
                )
                .children(
                    self.rate_error
                        .map(|error| kit::notice(Tone::Danger, tr(bardo, error), cx).text_xs()),
                )
                .into_any_element()
        } else {
            let edit_row = row.clone();
            let reset_key = key.clone();
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .text_sm()
                        .font_medium()
                        .child(SharedString::from(bardo.price(rate.price))),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(tr(bardo, Text::MeterUnit(rate.meter))),
                )
                .child(div().flex_1())
                .child(
                    Button::new(("edit-rate", ix))
                        .ghost()
                        .xsmall()
                        .label(tr(bardo, Text::EditRate))
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.edit_rate(&edit_row, window, cx)
                        })),
                )
                .when(row.changed, |actions| {
                    let label = match row.default {
                        Some(default) => SharedString::from(
                            bardo.text_with(Text::ResetRate, &[("price", &bardo.price(default))]),
                        ),
                        None => tr(bardo, Text::RemoveRate),
                    };
                    actions.child(
                        Button::new(("reset-rate", ix))
                            .ghost()
                            .xsmall()
                            .label(label)
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.reset_rate(reset_key.clone(), cx)
                            })),
                    )
                })
                .into_any_element()
        };

        h_flex()
            .px_3()
            .py_2()
            .gap_3()
            .items_center()
            .when(ix > 0, |row| row.border_t_1().border_color(theme.border))
            .child(
                v_flex()
                    .w(px(260.))
                    .flex_none()
                    .gap_0p5()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .text_sm()
                                    .font_medium()
                                    .child(tr(bardo, Text::ProviderName(rate.provider))),
                            )
                            .children(tag),
                    )
                    .child(div().text_xs().text_color(theme.muted_foreground).child(
                        SharedString::from(format!(
                            "{model} · {}",
                            bardo.text(Text::MeterName(rate.meter))
                        )),
                    )),
            )
            .child(div().flex_1().min_w_0().child(price))
            .into_any_element()
    }

    fn render_add_rate(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let bardo = self.bardo.read(cx);
        let paid: Vec<Provider> = Provider::paid().collect();
        let providers = ButtonGroup::new("new-rate-provider")
            .small()
            .outline()
            .children(paid.iter().map(|provider| {
                Button::new(("new-rate-provider", *provider as usize))
                    .label(tr(bardo, Text::ProviderName(*provider)))
                    .selected(*provider == self.new_provider)
            }))
            .on_click(cx.listener(move |this, clicked: &Vec<usize>, _, cx| {
                if let Some(provider) = clicked.first().and_then(|&i| paid.get(i)) {
                    this.new_provider = *provider;
                    this.add_error = None;
                    cx.notify();
                }
            }));
        let meters = ButtonGroup::new("new-rate-meter")
            .small()
            .outline()
            .children(Meter::ALL.map(|meter| {
                Button::new(("new-rate-meter", meter as usize))
                    .label(tr(bardo, Text::MeterName(meter)))
                    .selected(meter == self.new_meter)
            }))
            .on_click(cx.listener(|this, clicked: &Vec<usize>, _, cx| {
                if let Some(meter) = clicked.first().and_then(|&i| Meter::ALL.get(i)) {
                    this.new_meter = *meter;
                    this.add_error = None;
                    cx.notify();
                }
            }));

        kit::card(cx)
            .p_3()
            .gap_2()
            .child(div().font_medium().child(tr(bardo, Text::AddRateTitle)))
            .child(labeled(
                cx,
                tr(bardo, Text::RateProvider),
                providers.into_any_element(),
            ))
            .child(labeled(
                cx,
                tr(bardo, Text::RateModel),
                Input::new(&self.new_model).small().into_any_element(),
            ))
            .child(labeled(
                cx,
                tr(bardo, Text::RateMeter),
                meters.into_any_element(),
            ))
            .child(labeled(
                cx,
                SharedString::from(format!(
                    "{} ({})",
                    bardo.text(Text::RatePrice),
                    bardo.text(Text::MeterUnit(self.new_meter))
                )),
                div()
                    .w(px(160.))
                    .child(Input::new(&self.new_price).small())
                    .into_any_element(),
            ))
            .children(
                self.add_error
                    .map(|error| kit::notice(Tone::Danger, tr(bardo, error), cx).text_xs()),
            )
            .child(
                h_flex().child(
                    Button::new("add-rate")
                        .primary()
                        .small()
                        .label(tr(bardo, Text::AddRate))
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.add_rate(window, cx)
                        })),
                ),
            )
    }
}

/// A line of the budgets table: provider, usage, spent, budget, state and
/// actions, each in its column.
fn columns(
    provider: impl IntoElement,
    usage: impl IntoElement,
    spent: impl IntoElement,
    budget: impl IntoElement,
    state: impl IntoElement,
    actions: impl IntoElement,
) -> gpui_kit::Div {
    h_flex()
        .px_4()
        .gap_3()
        .items_center()
        .child(div().w(px(110.)).flex_none().child(provider))
        .child(div().flex_1().min_w(px(100.)).child(usage))
        .child(
            div()
                .w(px(72.))
                .flex_none()
                .flex()
                .justify_end()
                .child(spent),
        )
        .child(
            div()
                .w(px(72.))
                .flex_none()
                .flex()
                .justify_end()
                .child(budget),
        )
        .child(div().w(px(120.)).flex_none().child(state))
        .child(
            div()
                .w(px(84.))
                .flex_none()
                .flex()
                .justify_end()
                .child(actions),
        )
}

fn section_title(_cx: &App, text: SharedString) -> AnyElement {
    kit::section_heading(text).into_any_element()
}

fn hint(cx: &App, text: SharedString) -> AnyElement {
    div()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(text)
        .into_any_element()
}

fn labeled(cx: &App, label: SharedString, field: AnyElement) -> AnyElement {
    v_flex()
        .gap_1()
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(field)
        .into_any_element()
}

fn spend_line(
    cx: &App,
    name: SharedString,
    detail: Option<SharedString>,
    amount: String,
) -> AnyElement {
    h_flex()
        .gap_3()
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(div().text_sm().text_ellipsis().child(name))
                .children(detail.map(|detail| {
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(detail)
                })),
        )
        .child(
            div()
                .text_sm()
                .font_medium()
                .child(SharedString::from(amount)),
        )
        .into_any_element()
}

impl Render for CostsScreen {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let month = self.month_switch(cx);
        let summary = self.figures(cx);
        let budgets = self.budgets_table(cx);
        let rates = self.render_rates(cx).into_any_element();
        let aside = self.breakdown(cx);
        let on_pick = cx.listener(|this, index: &usize, _, cx| {
            this.tab = if *index == 0 {
                CostsTab::Budgets
            } else {
                CostsTab::Rates
            };
            cx.notify();
        });
        let bardo = self.bardo.read(cx);
        let mut header = Header::place(bardo, Destination::Costs);
        header.trail = vec![tr(bardo, Text::CostsOverview).into_any_element()];
        let info = guide::info(
            bardo,
            "costs-info",
            tr(bardo, Text::CostsHint),
            guide::refs::COSTS_MONTH,
        );
        // Costs always shows its providers and rates, so its tour is always
        // offered.
        header.info = guide::header_info(
            bardo,
            Destination::Costs,
            true,
            Some(info.into_any_element()),
            cx,
        );
        header.actions = vec![month];
        let mut parts = ScreenParts::new(header);
        parts.summary = summary;
        parts.sections = Some(Sections {
            id: "costs-tabs".into(),
            items: vec![
                Section {
                    title: tr(bardo, Text::CostsBudgetsTile),
                    body: budgets,
                },
                Section {
                    title: tr(bardo, Text::RatesTitle),
                    body: rates,
                },
            ],
            selected: match self.tab {
                CostsTab::Budgets => 0,
                CostsTab::Rates => 1,
            },
            on_pick: Rc::new(move |index, window, cx| on_pick(&index, window, cx)),
        });
        parts.notices =
            self.notice
                .map(|notice| kit::notice(Tone::Success, tr(bardo, notice), cx).into_any_element())
                .into_iter()
                .chain(self.error.map(|error| {
                    kit::notice(Tone::Danger, tr(bardo, error), cx).into_any_element()
                }))
                .collect();
        parts.aside = aside;
        parts.scroll = Some(self.scroll.clone());
        layout::screen(parts, cx)
    }
}
