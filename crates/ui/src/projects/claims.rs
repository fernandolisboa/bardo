//! The claims check of an ad's script, on the Script stage: what the
//! script states that no claim of the brief backs, the claims it uses,
//! and the button that checks the script again after an edit.

use bardo_app::bardo_domain::JobKind;
use bardo_app::{Bardo, BudgetConsent, ClaimsCheckView, ScriptError, Text};
use gpui_kit::component::button::Button;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, ClickEvent, Div, SharedString, Window, div};

use super::{ProjectsScreen, muted};
use crate::guide;
use crate::kit::{self, Tone};
use crate::shell::tr;
use crate::spend::{budget_question, estimate_note};

/// Whether a check found no statement about the product at all.
fn states_nothing(check: &ClaimsCheckView) -> bool {
    check.unbacked.is_empty() && check.used.is_empty() && check.sensitive.is_empty()
}

/// Where a check stands: out of date, or how much it found unbacked.
fn claims_status(bardo: &Bardo, check: &ClaimsCheckView, cx: &App) -> Div {
    if check.outdated {
        return kit::status(Tone::Warning, tr(bardo, Text::ScriptClaimsOutdated), cx);
    }
    if states_nothing(check) {
        // Nothing stated is nothing to back: neither good nor bad news.
        return kit::status(
            Tone::Neutral,
            tr(bardo, Text::ScriptClaimsNothingStated),
            cx,
        );
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

/// The line that says a check is being made.
fn claims_running(bardo: &Bardo) -> Div {
    h_flex()
        .gap_2()
        .items_center()
        .child(Spinner::new().small())
        .child(div().text_sm().child(tr(bardo, Text::ScriptClaimsRunning)))
}

/// What a check found: the sentences no claim backs, each with what it
/// states, then the claims the script uses, the sensitive ones apart.
/// `outdated_hint` says what to do about a check that is out of date.
fn claims_findings(bardo: &Bardo, check: &ClaimsCheckView, outdated_hint: Text, cx: &App) -> Div {
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
        // A flag the checker left without words still shows its sentence.
        let says = (!statement.says.trim().is_empty()).then(|| {
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(SharedString::from(statement.says.clone()))
        });
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
            .children(says)
    });
    v_flex()
        .gap_2()
        .when(check.outdated, |findings| {
            findings.child(kit::notice(Tone::Warning, tr(bardo, outdated_hint), cx).text_xs())
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
            // The chip already says so when the script states nothing.
            check.used.is_empty() && check.sensitive.is_empty() && !check.unbacked.is_empty(),
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
            Some(check) => {
                claims_findings(bardo, check, Text::ScriptClaimsOutdatedHint, cx).into_any_element()
            }
            None => muted(cx, tr(bardo, Text::ScriptClaimsNotChecked)),
        };
        // A check on request, or the check that follows the script this
        // job just wrote.
        let checking = self.running()
            && (self.job_wrote(false)
                || self
                    .view
                    .as_ref()
                    .and_then(|view| view.job.as_ref())
                    .is_some_and(|job| job.kind() == JobKind::ClaimsCheck));
        let running = checking.then(|| claims_running(bardo));
        // The check reads the script as saved, so edits come first.
        let unsaved = self
            .loaded
            .as_deref()
            .is_some_and(|saved| self.editor.read(cx).value().trim() != saved.trim());
        let again = (!self.running()).then(|| {
            let button = Button::new("check-claims")
                .small()
                .outline()
                .disabled(unsaved)
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
            let note = if unsaved {
                Some(
                    kit::notice(Tone::Info, tr(bardo, Text::ScriptClaimsSaveFirst), cx)
                        .text_xs()
                        .into_any_element(),
                )
            } else {
                estimate_note(bardo, &claims.estimate, Text::EstimateCost, cx)
                    .map(IntoElement::into_any_element)
            };
            v_flex()
                .gap_1()
                .child(h_flex().child(button))
                .children(note)
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
                .children(running)
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
            if !self.running() {
                return Some(muted(cx, tr(bardo, Text::ScriptClaimsPendingMissing)));
            }
            // The job that wrote the script is still checking it.
            return self
                .job_wrote(true)
                .then(|| claims_running(bardo).into_any_element());
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
                .child(claims_findings(
                    bardo,
                    check,
                    Text::ScriptClaimsPendingOutdatedHint,
                    cx,
                ))
                .into_any_element(),
        )
    }
}
