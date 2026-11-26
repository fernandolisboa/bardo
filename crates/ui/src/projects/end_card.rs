//! An ad's end card (#281) in the Scenes inspector: its headline and
//! second line as the user types them, the brief's offer on or off, its
//! look, and where its text and logo sit. Every choice composes the card
//! again, for free; the rules live in `bardo_app`.

use bardo_app::bardo_domain::{
    CardFieldError, CardLogoPlace, CardTextPlace, ThumbnailLook, VideoProjectId,
};
use bardo_app::{CardDraft, Control, Text, TourAnchor, card_field_errors};
use gpui_kit::component::button::Button;
use gpui_kit::component::input::Input;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Selectable as _, Sizable as _, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, ClickEvent, Window, div};

use super::{ProjectsScreen, muted};
use crate::guide;
use crate::kit::{self, Tone};
use crate::shell::tr;

impl ProjectsScreen {
    /// Reads the project's end card; its two lines are filled again only
    /// when the stored ones change, so typing stays.
    pub(super) fn load_end_card(
        &mut self,
        id: VideoProjectId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = self.bardo.read(cx).end_card(id).ok().flatten();
        if let Some(view) = &view {
            let stored = (
                view.design.headline().to_owned(),
                view.design.second_line().to_owned(),
            );
            if self.card_loaded.as_ref() != Some(&stored) {
                let (headline, second_line) = stored.clone();
                self.card_headline
                    .update(cx, |input, cx| input.set_value(headline, window, cx));
                self.card_second_line
                    .update(cx, |input, cx| input.set_value(second_line, window, cx));
                self.card_loaded = Some(stored);
            }
        } else {
            self.card_loaded = None;
        }
        self.end_card = view;
    }

    /// The design as on screen: the stored choices with the lines typed.
    fn card_draft(&self, cx: &App) -> Option<CardDraft> {
        let view = self.end_card.as_ref()?;
        Some(CardDraft {
            headline: self.card_headline.read(cx).value().to_string(),
            second_line: self.card_second_line.read(cx).value().to_string(),
            ..CardDraft::from(&view.design)
        })
    }

    /// Whether the lines hold words not saved yet.
    fn card_dirty(&self, cx: &App) -> bool {
        let headline = self.card_headline.read(cx).value().to_string();
        let second_line = self.card_second_line.read(cx).value().to_string();
        self.card_loaded.as_ref().is_some_and(|stored| {
            (headline.trim(), second_line.trim()) != (stored.0.as_str(), stored.1.as_str())
        })
    }

    /// Saves the design as `change` leaves the one on screen, and
    /// composes the card again.
    fn save_card(
        &mut self,
        change: impl FnOnce(&mut CardDraft),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(id), Some(mut draft)) = (self.project, self.card_draft(cx)) else {
            return;
        };
        change(&mut draft);
        match self.bardo.read(cx).save_end_card(id, &draft) {
            Ok(_) => {
                self.card_errors.clear();
                self.card_notice = Some(Text::EndCardSaved);
                self.scenes_error = None;
                // Shown again as stored: trimmed.
                self.card_loaded = None;
            }
            Err(error) => {
                self.card_errors = card_field_errors(&error).to_vec();
                self.card_notice = None;
                if self.card_errors.is_empty() {
                    self.scenes_error = Some(error.message());
                }
            }
        }
        self.load(window, cx);
        cx.notify();
    }

    /// The end card's editor, for scene `index` when it is the card.
    pub(super) fn render_end_card(
        &self,
        index: usize,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let view = self.end_card.as_ref()?;
        if view.scene != Some(index) {
            return None;
        }
        let locked = busy || self.scenes.as_ref().is_some_and(|view| view.is_animating());
        let design = view.design.clone();
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let error = |field: CardFieldError| -> Option<AnyElement> {
            self.card_errors.contains(&field).then(|| {
                div()
                    .text_xs()
                    .text_color(theme.danger)
                    .child(tr(bardo, Text::CardFieldError(field)))
                    .into_any_element()
            })
        };
        let headline_error =
            error(CardFieldError::HeadlineEmpty).or_else(|| error(CardFieldError::HeadlineTooLong));
        let choices = |label: Text, buttons: Vec<Button>| {
            v_flex()
                .gap_1()
                .child(super::scenes::field_label(tr(bardo, label)))
                .child(h_flex().flex_wrap().gap_1().children(buttons))
        };
        let look = choices(
            Text::EndCardLook,
            ThumbnailLook::ALL
                .into_iter()
                .map(|look| {
                    let shown = look == design.look;
                    Button::new(("card-look", look as usize))
                        .xsmall()
                        .outline()
                        .label(tr(bardo, Text::ThumbnailLookName(look)))
                        .selected(shown)
                        .disabled(locked && !shown)
                        .when(!shown, |button| {
                            button.on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                this.save_card(|draft| draft.look = look, window, cx);
                            }))
                        })
                })
                .collect(),
        );
        let text_place = choices(
            Text::EndCardTextPlace,
            CardTextPlace::ALL
                .into_iter()
                .map(|place| {
                    let shown = place == design.text_place;
                    Button::new(("card-text-place", place as usize))
                        .xsmall()
                        .outline()
                        .label(tr(bardo, Text::CardTextPlaceName(place)))
                        .selected(shown)
                        .disabled(locked && !shown)
                        .when(!shown, |button| {
                            button.on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                this.save_card(|draft| draft.text_place = place, window, cx);
                            }))
                        })
                })
                .collect(),
        );
        let logo_place = match view.has_logo {
            true => choices(
                Text::EndCardLogoPlace,
                CardLogoPlace::ALL
                    .into_iter()
                    .map(|place| {
                        let shown = place == design.logo_place;
                        Button::new(("card-logo-place", place as usize))
                            .xsmall()
                            .outline()
                            .label(tr(bardo, Text::CardLogoPlaceName(place)))
                            .selected(shown)
                            .disabled(locked && !shown)
                            .when(!shown, |button| {
                                button.on_click(cx.listener(
                                    move |this, _: &ClickEvent, window, cx| {
                                        this.save_card(
                                            |draft| draft.logo_place = place,
                                            window,
                                            cx,
                                        );
                                    },
                                ))
                            })
                    })
                    .collect(),
            )
            .into_any_element(),
            false => muted(cx, tr(bardo, Text::EndCardNoLogo)).into_any_element(),
        };
        // The offer line only ever shows the brief's offer.
        let offer = match view.offer.is_empty() {
            true => muted(cx, tr(bardo, Text::EndCardNoOffer)).into_any_element(),
            false => {
                let on = design.offer;
                v_flex()
                    .gap_1()
                    .items_start()
                    .child(
                        Button::new(("card-offer", index))
                            .xsmall()
                            .outline()
                            .label(tr(bardo, Text::EndCardShowOffer))
                            .selected(on)
                            .disabled(locked)
                            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                this.save_card(|draft| draft.offer = !on, window, cx);
                            })),
                    )
                    .child(muted(
                        cx,
                        bardo
                            .text(Text::EndCardOfferIs)
                            .replace("{offer}", &view.offer)
                            .into(),
                    ))
                    .into_any_element()
            }
        };
        let out_of_date = view.out_of_date.then(|| {
            kit::notice(Tone::Warning, tr(bardo, Text::EndCardOutOfDate), cx)
                .text_xs()
                .into_any_element()
        });
        let notice = self
            .card_notice
            .map(|notice| muted(cx, tr(bardo, notice)).into_any_element());
        let editor = v_flex()
            .gap_3()
            .child(
                kit::section_heading(tr(bardo, Text::EndCardTitle)).child(guide::info(
                    bardo,
                    ("end-card-info", index),
                    tr(bardo, Text::EndCardHint),
                    guide::refs::SCENES_END_CARD,
                )),
            )
            .children(out_of_date)
            .child(kit::field(
                tr(bardo, Text::EndCardHeadline),
                None,
                Input::new(&self.card_headline).into_any_element(),
                headline_error,
            ))
            .child(kit::field(
                tr(bardo, Text::EndCardSecondLine),
                Some(guide::info(
                    bardo,
                    ("end-card-second-line-info", index),
                    tr(bardo, Text::EndCardSecondLineHint),
                    guide::refs::SCENES_END_CARD,
                )),
                Input::new(&self.card_second_line).into_any_element(),
                error(CardFieldError::SecondLineTooLong),
            ))
            .child(
                h_flex().gap_2().items_center().child(
                    Button::new(("save-end-card", index))
                        .small()
                        .outline()
                        .label(tr(bardo, Text::EndCardSave))
                        .disabled(locked || !self.card_dirty(cx))
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.save_card(|_| {}, window, cx);
                        })),
                ),
            )
            .children(notice)
            .child(offer)
            .child(look)
            .child(text_place)
            .child(logo_place)
            .child(muted(cx, tr(bardo, Text::EndCardOff)));
        Some(kit::anchor(TourAnchor::Control(Control::EndCard), editor).into_any_element())
    }
}
