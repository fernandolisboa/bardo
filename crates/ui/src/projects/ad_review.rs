//! What an ad's post is reviewed with on the Publish stage (#282): the
//! check of its post texts against the brief's claims, the items the user
//! acknowledges before an upload or an export starts, and, in the upload
//! review, its commercial disclosure on the network.
//!
//! The acknowledgements are one list for the stage: an item ticked in the
//! export's card is ticked in every network's upload review too, and the
//! other way round. Each upload and export keeps the ones it went with.

use bardo_app::bardo_domain::{CommercialDisclosure, Network};
use bardo_app::{AdPublishing, Bardo, BudgetConsent, ExportError, Text, UploadReview};
use gpui_kit::component::button::{Button, ButtonGroup};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::Input;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Selectable as _, Sizable as _, StyledExt as _, h_flex,
    v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, ClickEvent, Div, SharedString, Window, div};

use super::{ProjectsScreen, muted};
use crate::guide;
use crate::kit::{self, Tone};
use crate::shell::tr;
use crate::spend::{budget_question, estimate_note};

/// A small heading inside a card.
fn heading(bardo: &Bardo, text: Text) -> Div {
    div().text_sm().font_medium().child(tr(bardo, text))
}

impl ProjectsScreen {
    /// The items of `ad` the user has yet to acknowledge.
    pub(super) fn ad_left(&self, ad: &AdPublishing) -> usize {
        ad.keys()
            .iter()
            .filter(|key| !self.ad_acknowledged.contains(key))
            .count()
    }

    fn acknowledge(&mut self, key: String, acknowledged: bool, cx: &mut Context<Self>) {
        self.ad_acknowledged.retain(|kept| *kept != key);
        if acknowledged {
            self.ad_acknowledged.push(key);
        }
        cx.notify();
    }

    fn acknowledge_all(&mut self, keys: Vec<String>, cx: &mut Context<Self>) {
        for key in keys {
            if !self.ad_acknowledged.contains(&key) {
                self.ad_acknowledged.push(key);
            }
        }
        cx.notify();
    }

    /// Starts a check of the post texts as they are saved.
    fn check_posts(&mut self, consent: BudgetConsent, window: &mut Window, cx: &mut Context<Self>) {
        self.posts_check_ask = None;
        let Some(id) = self.project else {
            return;
        };
        self.export_error = match self.bardo.read(cx).check_post_claims(id, consent) {
            Ok(_) => None,
            Err(ExportError::OverBudget(estimate)) => {
                self.posts_check_ask = Some(estimate);
                None
            }
            Err(error) => Some(error.message()),
        };
        self.export_notice = None;
        self.load(window, cx);
        cx.notify();
    }

    /// The items to acknowledge, each a box, with "Acknowledge all" while
    /// some are left. `id` keeps the boxes of two places apart.
    pub(super) fn ad_items(
        &self,
        id: &'static str,
        ad: &AdPublishing,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if ad.items.is_empty() {
            return None;
        }
        let bardo = self.bardo.read(cx);
        let screen = cx.entity().downgrade();
        let boxes = ad.items.iter().enumerate().map(|(i, item)| {
            let key = item.key();
            let checked = self.ad_acknowledged.contains(&key);
            let screen = screen.clone();
            Checkbox::new((id, i))
                .label(SharedString::from(bardo.ad_item_text(item)))
                .checked(checked)
                .on_click(move |checked: &bool, _, cx| {
                    let checked = *checked;
                    let key = key.clone();
                    let _ = screen.update(cx, |this, cx| this.acknowledge(key, checked, cx));
                })
        });
        let left = self.ad_left(ad);
        let keys = ad.keys();
        let all = (left > 0).then(|| {
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    Button::new((id, usize::MAX))
                        .small()
                        .outline()
                        .label(tr(bardo, Text::AdAcknowledgeAll))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.acknowledge_all(keys.clone(), cx);
                        })),
                )
                .child(muted(
                    cx,
                    SharedString::from(
                        bardo.text_with(Text::AdAcknowledgeLeft, &[("n", &left.to_string())]),
                    ),
                ))
        });
        Some(
            v_flex()
                .gap_2()
                .child(heading(bardo, Text::AdItemsHeading))
                .children(boxes)
                .children(all)
                .into_any_element(),
        )
    }

    /// The check of the post texts: where it stands, what no claim backs,
    /// and the button that checks them again.
    fn posts_check(&self, ad: &AdPublishing, writing: bool, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let title = h_flex()
            .gap_1()
            .items_center()
            .child(heading(bardo, Text::PostsCheckTitle))
            .child(guide::info(
                bardo,
                "posts-check-info",
                tr(bardo, Text::PostsCheckHint),
                guide::refs::UPLOADING_AD_REVIEW,
            ));
        let mut column = v_flex().gap_2().child(title);
        match &ad.posts {
            None => column = column.child(muted(cx, tr(bardo, Text::PostsCheckNone))),
            Some(check) => {
                if check.outdated {
                    column = column.child(
                        kit::notice(Tone::Warning, tr(bardo, Text::PostsCheckOutdated), cx)
                            .text_xs(),
                    );
                }
                if check.unbacked.is_empty() {
                    if !check.outdated {
                        column = column.child(kit::status(
                            Tone::Success,
                            tr(bardo, Text::PostsCheckAllBacked),
                            cx,
                        ));
                    }
                } else {
                    column = column.child(muted(cx, tr(bardo, Text::PostsCheckUnbacked)));
                    column = column.children(check.unbacked.iter().map(|statement| {
                        let says = (!statement.says.trim().is_empty()).then(|| {
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(SharedString::from(statement.says.clone()))
                        });
                        let sentence = statement
                            .sentence
                            .clone()
                            .unwrap_or_else(|| statement.says.clone());
                        kit::well(cx)
                            .py_2()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(kit::notice(Tone::Warning, SharedString::from(sentence), cx))
                            .children(says)
                    }));
                }
                column = column.child(div().text_xs().text_color(theme.muted_foreground).child(
                    SharedString::from(bardo.text_with(
                        Text::ScriptClaimsCheckedAt,
                        &[("when", &bardo.time_ago(check.checked_at))],
                    )),
                ));
            }
        }
        if ad.checking {
            column = column.child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(Spinner::new().small())
                    .child(div().text_sm().child(tr(bardo, Text::PostsCheckRunning))),
            );
        } else if ad.has_posts && !writing {
            // The check reads the texts as saved, so edits come first.
            let unsaved = self.metadata_dirty(cx);
            column = column.child(
                h_flex().child(
                    Button::new("posts-check-run")
                        .small()
                        .outline()
                        .disabled(unsaved)
                        .label(tr(bardo, Text::PostsCheckRun))
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.check_posts(BudgetConsent::Ask, window, cx)
                        })),
                ),
            );
            column = column.children(
                ad.check_estimate
                    .as_ref()
                    .and_then(|estimate| estimate_note(bardo, estimate, Text::EstimateCost, cx)),
            );
        }
        if let Some(estimate) = &self.posts_check_ask {
            column = column.child(budget_question(
                "posts-check-budget",
                bardo,
                estimate,
                cx,
                cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.check_posts(BudgetConsent::Confirmed, window, cx)
                }),
                cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.posts_check_ask = None;
                    cx.notify();
                }),
            ));
        }
        column.into_any_element()
    }

    /// The Publish stage's card of an ad: the check of its post texts and
    /// what the user acknowledges before an export or upload starts.
    pub(super) fn ad_review_card(
        &self,
        ad: &AdPublishing,
        writing: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let posts = self.posts_check(ad, writing, cx);
        let items = self.ad_items("ad-item", ad, cx);
        let bardo = self.bardo.read(cx);
        kit::card(cx)
            .p_3()
            .gap_3()
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(div().font_semibold().child(tr(bardo, Text::AdReviewTitle)))
                    .child(guide::info(
                        bardo,
                        "ad-review-info",
                        tr(bardo, Text::AdReviewHint),
                        guide::refs::UPLOADING_AD_REVIEW,
                    )),
            )
            .child(posts)
            .children(items)
            .into_any_element()
    }

    /// The commercial disclosure picked in the open upload review.
    pub(super) fn upload_disclosure(&self, ad: &AdPublishing) -> CommercialDisclosure {
        self.ad_disclosure.unwrap_or(ad.disclosure)
    }

    /// The upload review's commercial disclosure: the choice, what the
    /// network asks for it, and a paid Reel's brand accounts.
    pub(super) fn commercial_section(
        &self,
        review: &UploadReview,
        ad: &AdPublishing,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let picked = self.upload_disclosure(ad);
        let choice = ButtonGroup::new("upload-commercial")
            .outline()
            .small()
            .children(CommercialDisclosure::ALL.map(|disclosure| {
                Button::new(disclosure.code())
                    .label(tr(bardo, Text::CommercialName(disclosure)))
                    .selected(disclosure == picked)
            }))
            .on_click(cx.listener(|this, clicked: &Vec<usize>, _, cx| {
                if let Some(disclosure) = clicked
                    .first()
                    .and_then(|&i| CommercialDisclosure::ALL.get(i))
                {
                    this.ad_disclosure = Some(*disclosure);
                    this.sponsor_problem = None;
                    cx.notify();
                }
            }));
        let mut column = v_flex()
            .gap_1()
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        div()
                            .text_sm()
                            .font_medium()
                            .child(tr(bardo, Text::CommercialTitle)),
                    )
                    .child(guide::info(
                        bardo,
                        "upload-commercial-info",
                        tr(bardo, Text::CommercialHint),
                        guide::refs::UPLOADING_COMMERCIAL,
                    )),
            )
            .child(h_flex().child(choice));
        if let Some(step) = bardo.commercial_step_text(review.network, picked) {
            column = column.child(kit::notice(Tone::Info, SharedString::from(step), cx));
        }
        if picked != ad.disclosure {
            column = column.child(muted(cx, tr(bardo, Text::CommercialChangeNote)));
        }
        if review.network == Network::InstagramReels && picked.is_paid() {
            column = column.child(
                v_flex()
                    .gap_1()
                    .pt_1()
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(div().text_sm().child(tr(bardo, Text::SponsorsLabel)))
                            .child(guide::info(
                                bardo,
                                "upload-sponsors-info",
                                tr(bardo, Text::SponsorsHint),
                                guide::refs::INSTAGRAM_UPLOAD,
                            )),
                    )
                    .child(Input::new(&self.ad_sponsors).small())
                    .children(
                        self.sponsor_problem
                            .map(|problem| kit::notice(Tone::Danger, tr(bardo, problem), cx)),
                    ),
            );
        }
        column.into_any_element()
    }

    /// A YouTube post uploaded as a paid partnership: the box the user
    /// ticks in YouTube Studio, and the way there.
    pub(super) fn studio_note(&self, review: &UploadReview, cx: &App) -> Option<AnyElement> {
        let link = review.studio_link.clone()?;
        let bardo = self.bardo.read(cx);
        Some(
            v_flex()
                .gap_1()
                .child(kit::notice(
                    Tone::Info,
                    tr(bardo, Text::CommercialInStudio),
                    cx,
                ))
                .child(
                    h_flex().child(
                        Button::new("upload-studio-link")
                            .small()
                            .outline()
                            .label(tr(bardo, Text::StudioLink))
                            .on_click(move |_, _, cx| cx.open_url(&link)),
                    ),
                )
                .into_any_element(),
        )
    }
}
