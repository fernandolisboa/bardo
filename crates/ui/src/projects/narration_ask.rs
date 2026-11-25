//! What a new narration asks before it starts, whichever way it is made
//! (generated, an imported recording, a capture): whether to put the
//! scenes' paid images and clips out of date, then whether to go past a
//! budget.

use bardo_app::{BudgetConsent, PicturesAtStake, PicturesConsent, SpendEstimate, Text};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, h_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, ClickEvent, ElementId, SharedString, Window, div};

use super::ProjectsScreen;
use crate::kit;
use crate::shell::tr;
use crate::spend::budget_question;

/// How a new narration is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NarrationWay {
    Generate,
    Recording,
    Capture,
}

impl NarrationWay {
    fn id(self) -> &'static str {
        match self {
            NarrationWay::Generate => "narration",
            NarrationWay::Recording => "import",
            NarrationWay::Capture => "capture",
        }
    }
}

/// What a new narration waits for the user to answer.
#[derive(Debug, Clone)]
pub(super) enum NarrationAsk {
    /// The scenes hold images or clips, which it puts out of date.
    Pictures(PicturesAtStake),
    /// It reaches a budget. The answer about the pictures, given before
    /// with what the user saw, goes along when they confirm; the app asks
    /// again when something else is at stake by then.
    Budget(PicturesConsent, SpendEstimate),
}

impl ProjectsScreen {
    fn start_narration(
        &mut self,
        way: NarrationWay,
        pictures: PicturesConsent,
        consent: BudgetConsent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match way {
            NarrationWay::Generate => self.generate_narration(pictures, consent, window, cx),
            NarrationWay::Recording => self.import_recording(pictures, consent, window, cx),
            NarrationWay::Capture => self.import_capture(pictures, consent, window, cx),
        }
    }

    fn drop_narration_ask(&mut self, way: NarrationWay, cx: &mut Context<Self>) {
        match way {
            NarrationWay::Generate => {
                self.narration_ask = None;
                cx.notify();
            }
            NarrationWay::Recording => self.cancel_recording(cx),
            NarrationWay::Capture => self.cancel_capture(cx),
        }
    }

    /// The card that asks `ask` about a narration made `way`.
    pub(super) fn narration_question(
        &self,
        way: NarrationWay,
        ask: &NarrationAsk,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let id = way.id();
        let cancel = cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.drop_narration_ask(way, cx);
        });
        match ask {
            NarrationAsk::Budget(pictures, estimate) => {
                let pictures = *pictures;
                budget_question(
                    format!("{id}-budget"),
                    bardo,
                    estimate,
                    cx,
                    cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.start_narration(way, pictures, BudgetConsent::Confirmed, window, cx)
                    }),
                    cancel,
                )
            }
            NarrationAsk::Pictures(stake) => {
                let text = if stake.can_fit {
                    Text::NarrationOutdatesFit
                } else {
                    Text::NarrationOutdatesReplan
                };
                let question = bardo.text_with(
                    text,
                    &[
                        ("images", &stake.pictures.images.to_string()),
                        ("clips", &stake.pictures.clips.to_string()),
                    ],
                );
                // Fitting keeps the pictures; planning again loses them.
                let seen = *stake;
                let confirm = Button::new(ElementId::Name(format!("{id}-pictures-confirm").into()))
                    .small()
                    .label(tr(bardo, Text::NarrationOutdatesConfirm))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.start_narration(
                            way,
                            PicturesConsent::Confirmed(seen),
                            BudgetConsent::Ask,
                            window,
                            cx,
                        )
                    }));
                let confirm = if stake.can_fit {
                    confirm.primary()
                } else {
                    confirm.danger()
                };
                kit::card(cx)
                    .p_3()
                    .gap_2()
                    .border_color(cx.theme().warning)
                    .child(div().text_sm().child(SharedString::from(question)))
                    .child(
                        h_flex().gap_2().flex_wrap().child(confirm).child(
                            Button::new(ElementId::Name(format!("{id}-pictures-cancel").into()))
                                .ghost()
                                .small()
                                .label(tr(bardo, Text::NarrationOutdatesCancel))
                                .on_click(cancel),
                        ),
                    )
                    .into_any_element()
            }
        }
    }
}
