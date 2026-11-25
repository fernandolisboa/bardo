//! The Brief stage of an ad project: what this one video says about its
//! product. It shows the product and the angle the project was approved
//! from, the claims the video uses (each switchable off, and each one
//! that changed in the product since to keep, take again or leave out),
//! the offer, the call to action and the link of this video, and its
//! length. Confirming it opens the Script stage. Rules and storage live
//! in `bardo_app`; this file maps them to the screen.

use bardo_app::bardo_domain::{
    AdBrief, BriefFieldError, ClaimDrift, ClaimId, VideoLength, VideoProjectId,
};
use bardo_app::{BriefClaimRow, BriefError, BriefView, Control, Stage, Text, TourAnchor};
use gpui_kit::component::button::{Button, ButtonGroup, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Selectable as _, Sizable as _, StyledExt as _, h_flex,
    v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, ClickEvent, SharedString, Window, div, px};

use super::{ProjectsScreen, muted};
use crate::guide;
use crate::kit::{self, Tone};
use crate::shell::tr;

/// How wide the name of a part of the angle is, so the parts line up.
const PART_NAME_WIDTH: f32 = 92.;

/// The offer, the call to action and the link, as stored.
fn words(brief: &AdBrief) -> (String, String, String) {
    let text = brief.text();
    (
        text.offer().to_owned(),
        text.call_to_action().to_owned(),
        text.link().to_owned(),
    )
}

impl ProjectsScreen {
    pub(super) fn load_brief(
        &mut self,
        id: VideoProjectId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.bardo.read(cx).ad_brief(id) {
            Ok(Some(view)) => {
                // Refilled only when the stored words change, so typing
                // stays.
                let stored = words(&view.brief);
                if self.brief_loaded.as_ref() != Some(&stored) {
                    let (offer, call_to_action, link) = stored.clone();
                    self.brief_offer
                        .update(cx, |input, cx| input.set_value(offer, window, cx));
                    self.brief_call_to_action
                        .update(cx, |input, cx| input.set_value(call_to_action, window, cx));
                    self.brief_link
                        .update(cx, |input, cx| input.set_value(link, window, cx));
                    self.brief_loaded = Some(stored);
                }
                self.brief = Some(view);
            }
            Ok(None) => self.brief = None,
            Err(error) => {
                self.brief = None;
                self.brief_error = Some(error.message().unwrap_or(Text::BriefNotLoaded));
            }
        }
    }

    /// The words of the form as typed.
    fn brief_typed(&self, cx: &App) -> (String, String, String) {
        (
            self.brief_offer.read(cx).value().to_string(),
            self.brief_call_to_action.read(cx).value().to_string(),
            self.brief_link.read(cx).value().to_string(),
        )
    }

    /// Whether the form holds words not saved yet.
    fn brief_dirty(&self, cx: &App) -> bool {
        let (offer, call_to_action, link) = self.brief_typed(cx);
        self.brief_loaded.as_ref().is_some_and(|stored| {
            (offer.trim(), call_to_action.trim(), link.trim())
                != (stored.0.as_str(), stored.1.as_str(), stored.2.as_str())
        })
    }

    /// Shows how a change to the brief ended, and the brief as it is now.
    fn brief_changed(
        &mut self,
        changed: Result<AdBrief, BriefError>,
        done: Option<Text>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let ok = match changed {
            Ok(_) => {
                self.brief_errors.clear();
                self.brief_error = None;
                self.brief_notice = done;
                true
            }
            Err(error) => {
                self.brief_errors = error.field_errors().to_vec();
                self.brief_error = error.message();
                self.brief_notice = None;
                false
            }
        };
        self.load(window, cx);
        cx.notify();
        ok
    }

    /// Saves the offer, the call to action and the link. Returns whether
    /// they are saved.
    fn save_brief(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(id) = self.project else {
            return false;
        };
        let (offer, call_to_action, link) = self.brief_typed(cx);
        let saved = self
            .bardo
            .read(cx)
            .save_brief_text(id, &offer, &call_to_action, &link);
        if saved.is_ok() {
            // Shown again as stored: trimmed.
            self.brief_loaded = None;
        }
        self.brief_changed(saved, Some(Text::BriefSaved), window, cx)
    }

    /// Confirms the brief; words typed and not saved are saved first.
    fn confirm_brief(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.project else {
            return;
        };
        if self.brief_dirty(cx) && !self.save_brief(window, cx) {
            return;
        }
        let confirmed = self.bardo.read(cx).confirm_brief(id);
        self.brief_changed(confirmed, Some(Text::BriefConfirmedNotice), window, cx);
    }

    fn change_brief_claim(
        &mut self,
        change: fn(&bardo_app::Bardo, VideoProjectId, ClaimId) -> Result<AdBrief, BriefError>,
        claim: ClaimId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.project else {
            return;
        };
        let changed = change(self.bardo.read(cx), id, claim);
        self.brief_changed(changed, None, window, cx);
    }

    /// The user read the script against the changed brief and keeps it.
    pub(super) fn keep_script_after_brief(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.project else {
            return;
        };
        match self.bardo.read(cx).keep_script_after_brief(id) {
            Ok(_) => self.error = None,
            Err(error) => self.error = error.message(),
        }
        self.load(window, cx);
        cx.notify();
    }

    /// One part of the angle: its name, then what it says.
    fn brief_part(&self, name: Text, value: AnyElement, cx: &App) -> gpui_kit::Div {
        let bardo = self.bardo.read(cx);
        h_flex()
            .gap_2()
            .items_start()
            .child(
                div()
                    .flex_none()
                    .w(px(PART_NAME_WIDTH))
                    .pt_0p5()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr(bardo, name)),
            )
            .child(div().flex_1().min_w_0().text_sm().child(value))
    }

    /// The product and the angle the project was approved from.
    fn render_brief_angle(&self, view: &BriefView, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let text = |text: &str| SharedString::from(text.to_owned()).into_any_element();
        let product = v_flex()
            .gap_0p5()
            .child(SharedString::from(view.product.brief.name().to_owned()))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(tr(bardo, Text::ProductKindName(view.product.brief.kind()))),
            );
        let card = kit::card(cx)
            .p_4()
            .gap_2()
            .child(kit::section_heading(tr(bardo, Text::BriefAngleTitle)))
            .child(self.brief_part(Text::BriefProduct, product.into_any_element(), cx));
        let Some(angle) = view.angle.as_ref() else {
            return card.into_any_element();
        };
        let idea = angle.idea();
        let framework = idea.framework();
        let format = v_flex()
            .gap_0p5()
            .child(tr(bardo, Text::AdFrameworkName(framework)))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(tr(bardo, Text::AdFrameworkSteps(framework))),
            );
        let said = v_flex()
            .gap_0p5()
            .child(SharedString::from(idea.title().to_owned()))
            .when(!idea.angle().is_empty(), |said| {
                said.child(
                    div()
                        .text_color(theme.muted_foreground)
                        .child(SharedString::from(idea.angle().to_owned())),
                )
            });
        card.child(self.brief_part(Text::ThemeAngle, said.into_any_element(), cx))
            .child(self.brief_part(Text::AdAngleFramework, format.into_any_element(), cx))
            .when(!idea.segment().is_empty(), |card| {
                card.child(self.brief_part(Text::AdAngleSegment, text(idea.segment()), cx))
            })
            .child(self.brief_part(Text::AdAngleHook, text(idea.hook()), cx))
            .into_any_element()
    }

    /// One claim the video uses, with what to do about a change to it in
    /// the product.
    fn render_brief_claim(
        &self,
        ix: usize,
        row: &BriefClaimRow,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let id = row.copy.claim;
        let act = |name: &'static str,
                   label: Text,
                   change: fn(
            &bardo_app::Bardo,
            VideoProjectId,
            ClaimId,
        ) -> Result<AdBrief, BriefError>| {
            Button::new((name, ix))
                .small()
                .label(tr(bardo, label))
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.change_brief_claim(change, id, window, cx)
                }))
        };
        let leave_out = act(
            "brief-claim-off",
            Text::BriefClaimLeaveOut,
            |bardo, project, claim| bardo.set_brief_claim(project, claim, false),
        );
        let marks = h_flex()
            .flex_wrap()
            .gap_2()
            .items_center()
            .child(kit::status(
                Tone::Neutral,
                tr(bardo, Text::ClaimKindName(row.copy.kind)),
                cx,
            ))
            .when(row.copy.sensitive, |marks| {
                marks.child(kit::status(
                    Tone::Warning,
                    tr(bardo, Text::ClaimSensitive),
                    cx,
                ))
            })
            .when(row.in_angle, |marks| {
                marks.child(kit::status(
                    Tone::Info,
                    tr(bardo, Text::BriefClaimInAngle),
                    cx,
                ))
            });
        let well = kit::well(cx).p_3().gap_2().child(
            div()
                .text_sm()
                .child(SharedString::from(row.copy.text.clone())),
        );
        match &row.drift {
            None => well
                .child(
                    h_flex()
                        .flex_wrap()
                        .gap_3()
                        .items_center()
                        .justify_between()
                        .child(marks)
                        .child(leave_out.ghost()),
                )
                .into_any_element(),
            Some(drift) => {
                let (said, take) = match drift {
                    ClaimDrift::Edited { now } => (
                        v_flex()
                            .gap_1()
                            .child(kit::notice(
                                Tone::Warning,
                                tr(bardo, Text::BriefClaimEdited),
                                cx,
                            ))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme.muted_foreground)
                                    .child(SharedString::from(now.clone())),
                            ),
                        Some(
                            act(
                                "brief-claim-take",
                                Text::BriefClaimTake,
                                bardo_app::Bardo::update_brief_claim,
                            )
                            .outline(),
                        ),
                    ),
                    ClaimDrift::Unconfirmed => (
                        v_flex().child(kit::notice(
                            Tone::Warning,
                            tr(bardo, Text::BriefClaimUnconfirmed),
                            cx,
                        )),
                        None,
                    ),
                };
                well.child(marks)
                    .child(said)
                    .child(
                        h_flex()
                            .flex_wrap()
                            .gap_2()
                            .child(
                                act(
                                    "brief-claim-keep",
                                    Text::BriefClaimKeep,
                                    bardo_app::Bardo::keep_brief_claim,
                                )
                                .primary(),
                            )
                            .children(take)
                            .child(leave_out.outline()),
                    )
                    .into_any_element()
            }
        }
    }

    /// The claims the video uses, and the product's other confirmed
    /// claims it could use.
    fn render_brief_claims(&self, view: &BriefView, cx: &mut Context<Self>) -> AnyElement {
        let used: Vec<AnyElement> = view
            .claims
            .iter()
            .enumerate()
            .map(|(ix, row)| self.render_brief_claim(ix, row, cx))
            .collect();
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let none = used
            .is_empty()
            .then(|| muted(cx, tr(bardo, Text::BriefClaimsNone)));
        let available = view.available.iter().enumerate().map(|(ix, claim)| {
            let id = claim.id;
            kit::well(cx)
                .p_3()
                .child(
                    h_flex()
                        .gap_3()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_sm()
                                .text_color(theme.muted_foreground)
                                .child(SharedString::from(claim.text().to_owned())),
                        )
                        .child(
                            Button::new(("brief-claim-on", ix))
                                .small()
                                .outline()
                                .label(tr(bardo, Text::BriefClaimUse))
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    this.change_brief_claim(
                                        |bardo, project, claim| {
                                            bardo.set_brief_claim(project, claim, true)
                                        },
                                        id,
                                        window,
                                        cx,
                                    )
                                })),
                        ),
                )
                .into_any_element()
        });
        let left_out = (!view.available.is_empty()).then(|| {
            div()
                .pt_1()
                .text_sm()
                .font_medium()
                .child(SharedString::from(bardo.text_with(
                    Text::BriefClaimsLeftOut,
                    &[("n", &view.available.len().to_string())],
                )))
        });
        kit::card(cx)
            .p_4()
            .gap_3()
            .child(
                kit::section_heading(SharedString::from(bardo.text_with(
                    Text::BriefClaimsTitle,
                    &[("n", &view.claims.len().to_string())],
                )))
                .child(guide::info(
                    bardo,
                    "brief-claims-info",
                    tr(bardo, Text::BriefClaimsInfo),
                    guide::refs::BRIEF_CLAIMS,
                )),
            )
            .child(muted(cx, tr(bardo, Text::BriefClaimsHint)))
            .children(none)
            .children(used)
            .children(left_out)
            .children(available.collect::<Vec<_>>())
            .into_any_element()
    }

    /// The offer, the call to action, the link and the length of this
    /// video.
    fn render_brief_words(&self, view: &BriefView, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let error = |field: BriefFieldError| -> Option<AnyElement> {
            self.brief_errors.contains(&field).then(|| {
                div()
                    .text_xs()
                    .text_color(theme.danger)
                    .child(tr(bardo, Text::BriefFieldError(field)))
                    .into_any_element()
            })
        };
        let length = ButtonGroup::new("brief-length")
            .small()
            .outline()
            .children(VideoLength::ALL.map(|length| {
                Button::new(("brief-length-choice", length as usize))
                    .label(tr(bardo, Text::VideoLengthName(length)))
                    .selected(view.project.length == Some(length))
            }))
            .on_click(cx.listener(|this, clicked: &Vec<usize>, window, cx| {
                if let Some(length) = clicked.first().and_then(|&i| VideoLength::ALL.get(i)) {
                    this.set_length(Some(*length), window, cx);
                }
            }));
        kit::card(cx)
            .p_4()
            .gap_4()
            .child(
                kit::section_heading(tr(bardo, Text::BriefWordsTitle)).child(guide::info(
                    bardo,
                    "brief-words-info",
                    tr(bardo, Text::BriefWordsInfo),
                    guide::refs::BRIEF_WORDS,
                )),
            )
            .child(kit::field(
                tr(bardo, Text::ProductOffer),
                None,
                Input::new(&self.brief_offer).into_any_element(),
                error(BriefFieldError::OfferTooLong),
            ))
            .child(kit::field(
                tr(bardo, Text::ProductCallToAction),
                None,
                Input::new(&self.brief_call_to_action).into_any_element(),
                error(BriefFieldError::CallToActionTooLong),
            ))
            .child(kit::field(
                tr(bardo, Text::BriefLink),
                None,
                Input::new(&self.brief_link).into_any_element(),
                error(BriefFieldError::LinkTooLong),
            ))
            .child(
                h_flex().child(
                    Button::new("save-brief")
                        .small()
                        .outline()
                        .label(tr(bardo, Text::BriefSave))
                        .disabled(!self.brief_dirty(cx))
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.save_brief(window, cx);
                        })),
                ),
            )
            .child(kit::field(
                tr(bardo, Text::ProjectLength),
                Some(guide::info(
                    bardo,
                    "brief-length-info",
                    tr(bardo, Text::BriefLengthHint),
                    guide::refs::BRIEF_WORDS,
                )),
                h_flex().child(length).into_any_element(),
                None,
            ))
            .into_any_element()
    }

    /// Confirming the brief, or what stands in the way.
    fn render_brief_confirm(&self, view: &BriefView, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let not_ready = view
            .not_ready
            .map(|reason| kit::notice(Tone::Warning, tr(bardo, Text::BriefNotReady(reason)), cx));
        let notice = self
            .brief_notice
            .map(|notice| kit::notice(Tone::Success, tr(bardo, notice), cx));
        let error = self
            .brief_error
            .map(|error| kit::notice(Tone::Danger, tr(bardo, error), cx));
        let action = if view.standing.confirmed {
            h_flex()
                .flex_wrap()
                .gap_3()
                .items_center()
                .child(kit::status(
                    Tone::Success,
                    tr(bardo, Text::BriefConfirmed),
                    cx,
                ))
                .child(
                    Button::new("brief-to-script")
                        .small()
                        .outline()
                        .label(tr(bardo, Text::BriefToScript))
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.pick_stage(Stage::Script, cx)
                        })),
                )
        } else {
            h_flex()
                .gap_1()
                .items_center()
                .child(
                    Button::new("confirm-brief")
                        .primary()
                        .label(tr(bardo, Text::BriefConfirm))
                        .disabled(view.not_ready.is_some())
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.confirm_brief(window, cx)
                        })),
                )
                .child(guide::info(
                    bardo,
                    "confirm-brief-info",
                    tr(bardo, Text::BriefConfirmHint),
                    guide::refs::BRIEF_CONFIRM,
                ))
        };
        v_flex()
            .gap_3()
            .children(not_ready)
            .children(error)
            .children(notice)
            .child(action)
            .when(view.standing.confirmed, |column| {
                column.child(muted(cx, tr(bardo, Text::BriefAfterConfirm)))
            })
            .into_any_element()
    }

    /// The Brief stage of an ad project.
    pub(super) fn brief_page(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(view) = self.brief.clone() else {
            let bardo = self.bardo.read(cx);
            return vec![
                kit::notice(
                    Tone::Danger,
                    tr(bardo, self.brief_error.unwrap_or(Text::BriefNotLoaded)),
                    cx,
                )
                .into_any_element(),
            ];
        };
        let scroll = Some(&self.page_scroll);
        let tagged = |control: Control, element: AnyElement| {
            kit::anchor_in(TourAnchor::Control(control), element, scroll).into_any_element()
        };
        let angle = self.render_brief_angle(&view, cx);
        let claims = self.render_brief_claims(&view, cx);
        let words = self.render_brief_words(&view, cx);
        let confirm = self.render_brief_confirm(&view, cx);
        let bardo = self.bardo.read(cx);
        let title = v_flex()
            .gap_1()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(kit::section_heading(tr(
                        bardo,
                        Text::StageName(Stage::Brief),
                    )))
                    .child(guide::info(
                        bardo,
                        "brief-info",
                        tr(bardo, Text::BriefInfo),
                        guide::refs::BRIEF_ANGLE,
                    )),
            )
            .child(muted(cx, tr(bardo, Text::BriefHint)));
        vec![
            title.into_any_element(),
            tagged(Control::BriefAngle, angle),
            tagged(Control::BriefClaims, claims),
            tagged(Control::BriefWords, words),
            tagged(Control::BriefConfirm, confirm),
        ]
    }
}
