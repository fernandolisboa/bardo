//! The claims check of an ad's script, on the Script stage: what the
//! script states that no claim of the brief backs, the claims it uses,
//! and the button that checks the script again after an edit.

use bardo_app::{Bardo, BudgetConsent, ClaimsCheckView, ScriptError, Text};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, ClickEvent, Div, SharedString, Window, div};

use super::{ProjectsScreen, muted};
use crate::guide;
use crate::kit::{self, Tone};
use crate::shell::tr;
use crate::spend::{budget_question, estimate_note};

/// Where a check stands: out of date, or how much it found unbacked.
fn claims_status(bardo: &Bardo, check: &ClaimsCheckView, cx: &App) -> Div {
    if check.outdated {
        return kit::status(Tone::Warning, tr(bardo, Text::ScriptClaimsOutdated), cx);
    }
    match check.unbacked.len() {
        0 => kit::status(Tone::Success, tr(bardo, Text::ScriptClaimsClean), cx),
        1 => kit::status(Tone::Warning, tr(bardo, Text::ScriptClaimsUnbackedOne), cx),
        n => kit::status(
            Tone::Warning,
            SharedString::from(
                bardo.text_with(Text::ScriptClaimsUnbacked, &[("n", &n.to_string())]),
            ),
            cx,
        ),
    }
}

/// What a check found: the sentences no claim backs, each with what it
/// states, then the claims the script uses, the sensitive ones apart.
fn claims_findings(bardo: &Bardo, check: &ClaimsCheckView, cx: &App) -> Div {
    let theme = cx.theme();
    let label = |text: Text| div().text_sm().font_medium().child(tr(bardo, text));
    let lines = |claims: &[String]| {
        v_flex().gap_1().children(claims.iter().map(|claim| {
            h_flex()
                .gap_1p5()
                .items_start()
                .text_sm()
                .child(div().flex_none().child("•"))
                .child(div().min_w_0().child(SharedString::from(claim.clone())))
        }))
    };
    let unbacked = check.unbacked.iter().map(|statement| {
        let says = div()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(SharedString::from(statement.says.clone()));
        kit::well(cx)
            .py_2()
            .flex()
            .flex_col()
            .gap_1()
            .child(match &statement.sentence {
                Some(sentence) => {
                    kit::notice(Tone::Warning, SharedString::from(sentence.clone()), cx)
                }
                None => kit::notice(Tone::Warning, tr(bardo, Text::ScriptClaimsNoSentence), cx),
            })
            .child(says)
    });
    v_flex()
        .gap_2()
        .when(check.outdated, |findings| {
            findings.child(
                kit::notice(Tone::Warning, tr(bardo, Text::ScriptClaimsOutdatedHint), cx).text_xs(),
            )
        })
        .when(!check.unbacked.is_empty(), |findings| {
            findings
                .child(label(Text::ScriptClaimsUnbackedTitle))
                .children(unbacked)
        })
        .when(!check.used.is_empty(), |findings| {
            findings
                .child(label(Text::ScriptClaimsUsedTitle))
                .child(lines(&check.used))
        })
        .when(!check.sensitive.is_empty(), |findings| {
            findings
                .child(label(Text::ScriptClaimsSensitiveTitle))
                .child(lines(&check.sensitive))
        })
        .when(
            check.used.is_empty() && check.sensitive.is_empty(),
            |findings| findings.child(muted(cx, tr(bardo, Text::ScriptClaimsNoneUsed))),
        )
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(SharedString::from(format!(
                    "{} · {}",
                    bardo.text_with(
                        Text::ScriptClaimsCheckedAt,
                        &[("when", &bardo.time_ago(check.checked_at))],
                    ),
                    check.model
                ))),
        )
}

impl ProjectsScreen {
    /// Starts a check of the current script as it is saved.
    fn check_claims(
        &mut self,
        consent: BudgetConsent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.claims_ask = None;
        let Some(id) = self.project else {
            return;
        };
        self.error = match self.bardo.read(cx).check_script_claims(id, consent) {
            Ok(_) => None,
            Err(ScriptError::OverBudget(estimate)) => {
                self.claims_ask = Some(estimate);
                None
            }
            Err(error) => Some(error.message()),
        };
        self.notice = None;
        self.load(window, cx);
        cx.notify();
    }

    /// The current script's claims check; nothing for a project that is
    /// no ad, and before its script is written.
    pub(super) fn render_claims(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let claims = self.view.as_ref()?.claims.as_ref()?;
        let bardo = self.bardo.read(cx);
        let check = claims.current.as_ref();
        let title = h_flex()
            .gap_2()
            .items_center()
            .flex_wrap()
            .child(
                div()
                    .font_medium()
                    .child(tr(bardo, Text::ScriptClaimsTitle)),
            )
            .children(check.map(|check| claims_status(bardo, check, cx)))
            .child(guide::info(
                bardo,
                "claims-check-info",
                tr(bardo, Text::ScriptClaimsInfo),
                guide::refs::SCRIPT_CLAIMS,
            ));
        let findings = match check {
            Some(check) => claims_findings(bardo, check, cx).into_any_element(),
            None => muted(cx, tr(bardo, Text::ScriptClaimsNotChecked)),
        };
        let again = (!self.running()).then(|| {
            let button = Button::new("check-claims")
                .small()
                .label(tr(
                    bardo,
                    if check.is_some() {
                        Text::CheckScriptClaimsAgain
                    } else {
                        Text::CheckScriptClaims
                    },
                ))
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.check_claims(BudgetConsent::Ask, window, cx)
                }));
            // A check that still reads the script needs no other.
            let button = if check.is_some_and(|check| !check.outdated) {
                button.ghost()
            } else {
                button.outline()
            };
            v_flex()
                .gap_1()
                .child(h_flex().child(button))
                .children(estimate_note(
                    bardo,
                    &claims.estimate,
                    Text::EstimateCost,
                    cx,
                ))
        });
        let ask = self.claims_ask.as_ref().map(|estimate| {
            budget_question(
                "claims-budget",
                bardo,
                estimate,
                cx,
                cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.check_claims(BudgetConsent::Confirmed, window, cx)
                }),
                cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.claims_ask = None;
                    cx.notify();
                }),
            )
        });
        Some(
            kit::card(cx)
                .p_3()
                .gap_2()
                .child(title)
                .child(findings)
                .children(again)
                .children(ask)
                .into_any_element(),
        )
    }

    /// The claims check of the new script waiting for review, inside its
    /// card; nothing for a project that is no ad.
    pub(super) fn render_pending_claims(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let claims = self.view.as_ref()?.claims.as_ref()?;
        let bardo = self.bardo.read(cx);
        let Some(check) = claims.pending.as_ref() else {
            // The job that wrote the script is still checking it.
            return (!self.running())
                .then(|| muted(cx, tr(bardo, Text::ScriptClaimsPendingMissing)));
        };
        Some(
            v_flex()
                .gap_2()
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .flex_wrap()
                        .child(
                            div()
                                .text_sm()
                                .font_medium()
                                .child(tr(bardo, Text::ScriptClaimsTitle)),
                        )
                        .child(claims_status(bardo, check, cx)),
                )
                .child(claims_findings(bardo, check, cx))
                .into_any_element(),
        )
    }
}
