//! The Scenes and Clips stages of the projects screen: plan the
//! narration's scenes, read and edit each scene's image prompt, draw the
//! images, draw one again and choose between the current image and the new
//! one; see the recurring people a scene shows with their reference
//! portraits, edit how one looks or draw their portrait again. Then
//! animate the images into clips: pick each scene's video model,
//! edit how it moves, and play, keep or discard each new clip. The scenes
//! are a grid of cards; the picked one opens in the inspector. Clips play in the system's
//! video player until the editor has its own.

use bardo_app::bardo_domain::{
    AnimatedScenes, CastMember, ClipModel, ClipModelRef, Job, JobState, Scene, SceneClip,
    SceneImage, TemplateKind, VideoProjectId,
};
use std::rc::Rc;

use bardo_app::{
    Bardo, BudgetConsent, Control, SceneClipView, SceneError, SceneState, ScenesView, Stage, Step,
    Text, TourAnchor, scene_states, step_selection,
};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Textarea;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, StyledExt as _, h_flex,
    v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, ClickEvent, ObjectFit, SharedString, Window, div, img, px};

use super::{ProjectsScreen, PromptShown, clock, muted};
use crate::appearance::look;
use crate::guide;
use crate::icons::Lucide;
use crate::kit::{self, Tone};
use crate::parts::{
    Collection, CollectionKeys, CollectionKind, Fact, Headings, Inspector, ScreenParts, Tile,
};
use crate::shell::tr;
use crate::spend::{budget_question, estimate_note};

/// A scene action that starts a paid job.
#[derive(Clone, Copy)]
pub(super) enum SceneAction {
    Plan {
        discard_images: bool,
    },
    DrawMissing,
    Redraw(usize),
    /// Draw a cast member's portrait again.
    RedrawPortrait(usize),
    /// Draw again every scene drawn in another style than the video's.
    RedrawRestyled,
    AnimateMissing,
    Animate(usize),
}

/// Which prompt of a scene the editor holds.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ScenePromptField {
    Image,
    Motion,
    /// A cast member's description.
    Cast(usize),
    /// A place's description.
    Place(usize),
}

impl ProjectsScreen {
    pub(super) fn load_scenes(&mut self, id: VideoProjectId, cx: &mut Context<Self>) {
        match self.bardo.read(cx).scenes(id) {
            Ok(view) => {
                // The scene being edited went with a new plan.
                let plan = view.plan.as_ref().map(|plan| plan.id);
                if self
                    .editing_scene
                    .is_some_and(|(id, _, _)| Some(id) != plan)
                {
                    self.editing_scene = None;
                    self.scene_field_error = None;
                }
                self.scenes = Some(view);
            }
            Err(error) => {
                self.scenes = None;
                self.scenes_error = Some(error.message());
            }
        }
    }

    /// Runs a scene action and shows what went wrong, if anything.
    fn scene_action<T>(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        action: impl FnOnce(&bardo_app::Bardo, VideoProjectId) -> Result<T, SceneError>,
    ) -> Option<T> {
        let id = self.project?;
        let result = action(self.bardo.read(cx), id);
        self.scenes_error = result.as_ref().err().map(SceneError::message);
        self.load(window, cx);
        cx.notify();
        result.ok()
    }

    /// Starts a paid scene job. Discarding images and going past a budget
    /// are asked in the panel instead of shown as errors.
    fn run_scene(
        &mut self,
        action: SceneAction,
        consent: BudgetConsent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.confirm_replan = false;
        self.scenes_ask = None;
        let Some(id) = self.project else {
            return;
        };
        let bardo = self.bardo.read(cx);
        let result = match action {
            SceneAction::Plan { discard_images } => bardo.plan_scenes(id, discard_images, consent),
            SceneAction::DrawMissing => bardo.generate_scene_images(id, consent),
            SceneAction::Redraw(index) => bardo.regenerate_scene_image(id, index, consent),
            SceneAction::RedrawPortrait(member) => bardo.redraw_cast_portrait(id, member, consent),
            SceneAction::RedrawRestyled => bardo.redraw_restyled_scenes(id, consent),
            SceneAction::AnimateMissing => bardo.generate_missing_clips(id, consent),
            SceneAction::Animate(index) => bardo.generate_scene_clip(id, index, consent),
        };
        self.scenes_error = match result {
            Ok(_) => None,
            Err(SceneError::WouldDiscardImages(_)) => {
                self.confirm_replan = true;
                None
            }
            Err(SceneError::OverBudget(estimate)) => {
                self.scenes_ask = Some((action, estimate));
                None
            }
            Err(error) => Some(error.message()),
        };
        self.load(window, cx);
        cx.notify();
    }

    fn edit_scene(
        &mut self,
        index: usize,
        field: ScenePromptField,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(plan) = self.scenes.as_ref().and_then(|view| view.plan.as_ref()) else {
            return;
        };
        let Ok(scene) = plan.scene(index) else {
            return;
        };
        // A motion prompt the scene does not have opens with the planned
        // one, or empty in plans without one: saving it unchanged or empty
        // keeps following it.
        let text = match field {
            ScenePromptField::Image => scene.prompt().as_str().to_owned(),
            ScenePromptField::Motion => scene
                .own_motion_prompt()
                .or(scene.planned_motion())
                .map(|prompt| prompt.as_str().to_owned())
                .unwrap_or_default(),
            ScenePromptField::Cast(member) => match plan.cast_member(member) {
                Ok(member) => member.description().as_str().to_owned(),
                Err(_) => return,
            },
            ScenePromptField::Place(place) => match plan.place(place) {
                Ok(place) => place.description().as_str().to_owned(),
                Err(_) => return,
            },
        };
        self.editing_scene = Some((plan.id, index, field));
        self.scene_field_error = None;
        self.scene_editor
            .update(cx, |input, cx| input.set_value(text, window, cx));
        cx.notify();
    }

    fn save_scene_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, index, field)) = self.editing_scene else {
            return;
        };
        let text = self.scene_editor.read(cx).value();
        let saved = self.scene_action(window, cx, |bardo, id| match field {
            ScenePromptField::Image => bardo.edit_scene_prompt(id, index, &text),
            ScenePromptField::Motion => bardo.edit_scene_motion_prompt(id, index, &text),
            ScenePromptField::Cast(member) => bardo.edit_cast_description(id, member, &text),
            ScenePromptField::Place(place) => bardo.edit_place_description(id, place, &text),
        });
        match saved {
            Some(_) => {
                self.editing_scene = None;
                self.scene_field_error = None;
            }
            None => {
                // A field problem is shown under the editor.
                if let Some(Text::SceneFieldError(error)) = self.scenes_error {
                    self.scene_field_error = Some(error);
                    self.scenes_error = None;
                }
            }
        }
    }

    /// The Scenes and Clips stages: the toolbar, what happened, the scenes
    /// as a grid of cards, and the picked scene in the inspector.
    pub(super) fn scene_parts(
        &self,
        stage: Stage,
        parts: &mut ScreenParts,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self.scenes.as_ref() else {
            let bardo = self.bardo.read(cx);
            parts.notices.extend(
                self.scenes_error.map(|error| {
                    kit::notice(Tone::Danger, tr(bardo, error), cx).into_any_element()
                }),
            );
            return;
        };
        let states = scene_states(view, stage);
        let selected = self.shown_scene(&states);
        parts.toolbar = Some(self.scenes_toolbar(stage, view, &states, cx));
        parts.notices.extend(self.scenes_notices(stage, view, cx));
        parts.collection = Some(self.scene_grid(stage, view, &states, selected, cx));
        parts.inspector = view
            .plan
            .as_ref()
            .and(selected)
            .map(|index| self.scene_inspector(stage, index, cx));
        if let Some(plan) = &view.plan {
            parts.content.push(self.render_provenance(
                &plan.generation,
                TemplateKind::ImagePrompt,
                PromptShown::ScenePlan,
                cx,
            ));
        }
    }

    /// The scene in the inspector: the picked one, else the first with
    /// something left, else the first.
    fn shown_scene(&self, states: &[SceneState]) -> Option<usize> {
        self.selected_scene
            .filter(|index| *index < states.len())
            .or_else(|| states.iter().position(|state| state.is_pending()))
            .or((!states.is_empty()).then_some(0))
    }

    /// The scenes on screen, in order: every one, or those with something
    /// left.
    fn shown_scenes(&self, states: &[SceneState]) -> Vec<usize> {
        (0..states.len())
            .filter(|index| !self.pending_only || states[*index].is_pending())
            .collect()
    }

    /// ↑/↓ over the scenes: the inspector follows the selection.
    fn step_scene(&mut self, stage: Stage, step: Step, cx: &mut Context<Self>) {
        let Some(view) = self.scenes.as_ref() else {
            return;
        };
        let states = scene_states(view, stage);
        let shown = self.shown_scenes(&states);
        if let Some(index) = step_selection(&shown, self.shown_scene(&states), step) {
            self.selected_scene = Some(index);
            if let Some(row) = shown.iter().position(|shown| *shown == index) {
                self.scene_scroll.scroll_to_item(row);
            }
            cx.notify();
        }
    }

    /// Enter over the scenes: the selected scene's new image (Scenes) or
    /// clip (Clips) replaces the current one, when one waits for review.
    fn accept_shown(&mut self, stage: Stage, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = self.scenes.as_ref() else {
            return;
        };
        let states = scene_states(view, stage);
        let Some(index) = self.shown_scene(&states) else {
            return;
        };
        if states[index] != SceneState::Review {
            return;
        }
        if stage == Stage::Clips {
            self.scene_action(window, cx, |bardo, id| bardo.accept_scene_clip(id, index));
        } else {
            self.scene_action(window, cx, |bardo, id| bardo.accept_scene_image(id, index));
        }
    }

    /// The filter, then what the stage does to every scene: plan them and
    /// draw the missing images, or animate the missing clips.
    fn scenes_toolbar(
        &self,
        stage: Stage,
        view: &ScenesView,
        states: &[SceneState],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (actions, estimate) = if stage == Stage::Clips {
            self.clips_actions(view, cx)
        } else {
            self.scenes_actions(view, cx)
        };
        let bardo = self.bardo.read(cx);
        let pending = states.iter().filter(|state| state.is_pending()).count();
        // A version's carried scenes say so in their own card.
        let can_place = view
            .version
            .as_ref()
            .is_some_and(|version| version.can_place);
        let row = h_flex();
        let filter = (!states.is_empty()).then(|| {
            kit::anchor(
                TourAnchor::Control(Control::ScenesFilter),
                TabBar::new("scene-filter")
                    .segmented()
                    .small()
                    .selected_index(usize::from(self.pending_only))
                    .child(Tab::new().label(SharedString::from(format!(
                        "{} ({})",
                        bardo.text(Text::FilterAll),
                        states.len()
                    ))))
                    .child(Tab::new().label(SharedString::from(format!(
                        "{} ({pending})",
                        bardo.text(Text::FilterPending)
                    ))))
                    .on_click(cx.listener(|this, index: &usize, _, cx| {
                        this.pending_only = *index == 1;
                        cx.notify();
                    })),
            )
        });
        // What the stage does to every scene, as one control a tour lights.
        let control = if stage == Stage::Clips {
            TourAnchor::Control(Control::ClipsAnimate)
        } else {
            TourAnchor::Control(Control::ScenesPlan)
        };
        let actions = kit::anchor(
            control,
            h_flex()
                .gap_2()
                .flex_wrap()
                .items_center()
                .children(actions),
        );
        let row = row
            .gap_2()
            .flex_wrap()
            .items_center()
            .children(filter)
            // Scenes without a narration yet (a new version's) say so in
            // their own notice.
            .when(
                view.narration.is_some() && view.stale && !can_place,
                |row| {
                    // Fitting keeps the pictures; else only planning again.
                    let (why, page) = if view.can_fit {
                        (Text::ScenesStaleFit, guide::refs::SCENES_FIT)
                    } else {
                        (Text::ScenesStale, guide::refs::SCENES_REPLAN)
                    };
                    row.child(kit::status(
                        Tone::Warning,
                        tr(bardo, Text::ScenesStaleTag),
                        cx,
                    ))
                    .child(guide::info(
                        bardo,
                        "scenes-stale-info",
                        tr(bardo, why),
                        page,
                    ))
                },
            )
            .child(div().flex_1())
            .child(actions);
        // The channel's model was retired by its provider: say which one
        // makes the clips now.
        let replaced = (stage == Stage::Clips)
            .then(|| {
                let old = view.clips.default_replaces.as_ref()?;
                let new = view.clips.default_model.as_ref()?;
                let text = bardo.text_with(
                    Text::ClipsChannelModelReplaced,
                    &[("old", old), ("new", &new.name)],
                );
                Some(kit::notice(Tone::Info, SharedString::from(text), cx).text_xs())
            })
            .flatten();
        // The leading action's estimate, under it.
        v_flex()
            .gap_1()
            .child(row)
            .children(estimate.map(|estimate| h_flex().justify_end().child(estimate)))
            .children(replaced)
            .into_any_element()
    }

    /// Plan (again) and draw the missing images, and the leading one's
    /// estimate.
    fn scenes_actions(
        &self,
        view: &ScenesView,
        cx: &mut Context<Self>,
    ) -> (Vec<AnyElement>, Option<AnyElement>) {
        let bardo = self.bardo.read(cx);
        let busy = view.is_busy();
        let plan = view.plan.as_ref();
        let missing = view.missing_images();
        let stale = view.stale;
        // Only the leading button is primary: planning while there is no
        // plan or it is stale and cannot be fitted, else drawing the
        // missing images.
        let can_place = view
            .version
            .as_ref()
            .is_some_and(|version| version.can_place);
        let can_plan = !busy && !view.is_animating() && view.narration.is_some() && !view.too_long;
        // A plan that cannot be asked for leads nothing; nor does one whose
        // scenes can be fitted, which keeps their pictures.
        let plans_first = (plan.is_none() || (stale && !can_place && !view.can_fit)) && can_plan;
        let plan_button = can_plan.then(|| {
            let button = Button::new("plan-scenes").small().label(tr(
                bardo,
                if plan.is_some() {
                    Text::ReplanScenes
                } else {
                    Text::PlanScenes
                },
            ));
            if plans_first {
                button.primary()
            } else {
                button.outline()
            }
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                this.run_scene(
                    SceneAction::Plan {
                        discard_images: false,
                    },
                    BudgetConsent::Ask,
                    window,
                    cx,
                )
            }))
            .into_any_element()
        });
        let draw_button = (missing > 0 && !busy).then(|| {
            let button = self.paid(Button::new("generate-scene-images").small(), cx);
            if plans_first || stale {
                button.outline()
            } else {
                button.primary()
            }
            .label(SharedString::from(bardo.text_with(
                Text::GenerateSceneImages,
                &[("n", &missing.to_string())],
            )))
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                this.run_scene(SceneAction::DrawMissing, BudgetConsent::Ask, window, cx);
            }))
            .into_any_element()
        });
        let hint = match (view.narration, plan) {
            (None, _) => tr(bardo, Text::ScenesNoNarration),
            // With a plan the notice above says it; the hint is about the
            // images still to draw.
            (Some(_), None) if view.too_long => tr(bardo, Text::ScenesNarrationTooLong),
            (Some(_), None) => SharedString::from(bardo.text_with(
                Text::PlanScenesHint,
                &[("n", &view.template.number.to_string())],
            )),
            (Some(_), Some(_)) => tr(bardo, Text::SceneImagesHint),
        };
        // The estimate of the button that leads.
        let estimate = if plan_button.is_none() && draw_button.is_none() {
            None
        } else if plans_first {
            view.plan_estimate.as_ref()
        } else if missing > 0 {
            view.images_estimate.as_ref()
        } else {
            None
        }
        .and_then(|estimate| estimate_note(bardo, estimate, Text::EstimateCost, cx));
        let actions = plan_button
            .into_iter()
            .chain(draw_button)
            .chain(Some(
                guide::info(bardo, "scenes-info", hint, guide::refs::SCENES_PLAN)
                    .into_any_element(),
            ))
            .collect();
        (actions, estimate)
    }

    /// Animate every scene without a clip, and its estimate.
    fn clips_actions(
        &self,
        view: &ScenesView,
        cx: &mut Context<Self>,
    ) -> (Vec<AnyElement>, Option<AnyElement>) {
        let bardo = self.bardo.read(cx);
        let missing = view.clips.missing.len();
        // The mode says which scenes the button animates (#248).
        let label = match view.clips.mode.animated() {
            AnimatedScenes::Hook => Text::AnimateHookClips,
            AnimatedScenes::None | AnimatedScenes::All => Text::AnimateMissingClips,
        };
        let button = (missing > 0 && !view.is_busy()).then(|| {
            self.paid(Button::new("animate-missing-clips"), cx)
                .primary()
                .small()
                .label(SharedString::from(
                    bardo.text_with(label, &[("n", &missing.to_string())]),
                ))
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.run_scene(SceneAction::AnimateMissing, BudgetConsent::Ask, window, cx);
                }))
                .into_any_element()
        });
        let economy = view.plan.is_some() && view.clips.mode.animated() == AnimatedScenes::None;
        let estimate = match &button {
            Some(_) => view
                .clips
                .missing_estimate
                .as_ref()
                .and_then(|estimate| estimate_note(bardo, estimate, Text::EstimateCost, cx)),
            None if economy => Some(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr(bardo, Text::ClipsEconomy))
                    .into_any_element(),
            ),
            None => None,
        };
        let actions = button
            .into_iter()
            .chain(Some(
                guide::info(
                    bardo,
                    "clips-info",
                    tr(bardo, Text::ClipsHint),
                    guide::refs::CLIPS_ANIMATE,
                )
                .into_any_element(),
            ))
            .collect();
        (actions, estimate)
    }

    /// A button that pays for an image or a clip: off, and saying why,
    /// while the scenes are out of date (#262).
    fn paid(&self, button: Button, cx: &App) -> Button {
        let Some(view) = self.scenes.as_ref().filter(|view| view.stale) else {
            return button;
        };
        let placed = view
            .version
            .as_ref()
            .is_some_and(|version| version.can_place);
        // What brings the scenes onto the narration, by what the stage
        // offers: a narration first, placing, fitting, or planning again.
        let why = if view.narration.is_none() {
            Text::ScenesNoNarration
        } else if placed {
            Text::ScenesPaidHeldVersion
        } else if view.can_fit {
            Text::ScenesPaidHeld
        } else {
            Text::ScenesPaidHeldReplan
        };
        button.disabled(true).tooltip(tr(self.bardo.read(cx), why))
    }

    fn fit_scenes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The scenes are kept: a question about replacing them is over.
        self.confirm_replan = false;
        self.scenes_ask = None;
        let Some(id) = self.project else {
            return;
        };
        self.scenes_error = self
            .bardo
            .read(cx)
            .fit_scenes(id)
            .err()
            .map(|error| error.message());
        self.load(window, cx);
        cx.notify();
    }

    /// After a new narration of the same text: fitting the scenes to it,
    /// which keeps every image and clip.
    fn render_fit_scenes(&self, view: &ScenesView, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !view.can_fit {
            return None;
        }
        let bardo = self.bardo.read(cx);
        let busy = view.is_busy() || view.is_animating();
        let card = kit::card(cx)
            .px_3()
            .py_2()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap_2()
            .items_center()
            .border_color(look(cx).tokens.accent_edge)
            .child(
                h_flex()
                    .flex_1()
                    .gap_1()
                    .items_center()
                    .child(div().font_medium().child(tr(bardo, Text::FitScenesTitle)))
                    .child(guide::info(
                        bardo,
                        "fit-scenes-info",
                        tr(bardo, Text::FitScenesHint),
                        guide::refs::SCENES_FIT,
                    )),
            )
            .when(!busy, |card| {
                card.child(
                    Button::new("fit-scenes")
                        .primary()
                        .small()
                        .label(tr(bardo, Text::FitScenes))
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.fit_scenes(window, cx)
                        })),
                )
            });
        Some(card.into_any_element())
    }

    fn place_scenes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.project else {
            return;
        };
        self.scenes_error = self
            .bardo
            .read(cx)
            .place_scenes(id)
            .err()
            .map(|error| error.message());
        self.load(window, cx);
        cx.notify();
    }

    /// On a version: placing the original's scenes on its narration, or,
    /// once placed, the scenes worth a look.
    fn render_place_scenes(&self, view: &ScenesView, cx: &mut Context<Self>) -> Option<AnyElement> {
        let version = view.version.as_ref()?;
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let seconds = |duration: std::time::Duration| format!("{:.1}", duration.as_secs_f64());
        if version.can_place {
            let busy = view.is_busy() || view.is_animating();
            // One line, the explanation behind its ⓘ: the scene list keeps
            // its room in a short window.
            let card = kit::card(cx)
                .px_3()
                .py_2()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap_2()
                .items_center()
                .border_color(look(cx).tokens.accent_edge)
                .child(
                    h_flex()
                        .flex_1()
                        .gap_1()
                        .items_center()
                        .child(div().font_medium().child(tr(bardo, Text::PlaceScenesTitle)))
                        .child(guide::info(
                            bardo,
                            "place-scenes-info",
                            tr(bardo, Text::PlaceScenesHint),
                            guide::refs::SCENES_PLACE,
                        )),
                )
                .when(!busy, |card| {
                    card.child(
                        Button::new("place-scenes")
                            .primary()
                            .small()
                            .label(tr(bardo, Text::PlaceScenes))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.place_scenes(window, cx)
                            })),
                    )
                });
            return Some(
                kit::anchor(TourAnchor::Control(Control::ScenesPlace), card).into_any_element(),
            );
        }
        if version.notes.is_empty() {
            return None;
        }
        let plan = view.plan.as_ref()?;
        let lines: Vec<SharedString> = version
            .notes
            .iter()
            .filter_map(|(index, note)| {
                let scene = plan.scenes().get(*index)?;
                let label = bardo.text_with(
                    Text::SceneLabel,
                    &[
                        ("n", &(index + 1).to_string()),
                        ("start", &clock(scene.start)),
                        ("end", &clock(scene.end)),
                    ],
                );
                let what = match note {
                    bardo_app::bardo_domain::PlacedScene::PastClip { clip, .. } => {
                        bardo.text_with(Text::PlacedScenePastClip, &[("clip", &seconds(*clip))])
                    }
                    bardo_app::bardo_domain::PlacedScene::Stretched { was, now } => bardo
                        .text_with(
                            Text::PlacedSceneStretched,
                            &[("was", &seconds(*was)), ("now", &seconds(*now))],
                        ),
                    bardo_app::bardo_domain::PlacedScene::Dropped => {
                        bardo.text(Text::PlacedSceneDropped).into_owned()
                    }
                };
                Some(SharedString::from(format!("{label}: {what}")))
            })
            .collect();
        // A few lines show; past that, all of them behind a button, so the
        // scene list keeps its room.
        const SHOWN: usize = 3;
        let more = (lines.len() > SHOWN).then(|| {
            h_flex().child(kit::details(
                "placed-scenes-all",
                SharedString::from(
                    bardo.text_with(Text::PlacedScenesAll, &[("n", &lines.len().to_string())]),
                ),
                lines.clone(),
            ))
        });
        let card = kit::card(cx)
            .px_3()
            .py_2()
            .gap_1()
            .border_color(theme.warning)
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        div()
                            .font_medium()
                            .child(tr(bardo, Text::PlacedScenesTitle)),
                    )
                    .child(guide::info(
                        bardo,
                        "placed-scenes-info",
                        tr(bardo, Text::PlaceScenesHint),
                        guide::refs::SCENES_PLACE,
                    )),
            )
            .children(
                lines
                    .into_iter()
                    .take(SHOWN)
                    .map(|line| div().text_sm().child(line)),
            )
            .children(more);
        Some(kit::anchor(TourAnchor::Control(Control::ScenesPlace), card).into_any_element())
    }

    /// The scenes whose image is in another style than the video's (#259):
    /// how many, why it matters behind the ⓘ, and one button that draws
    /// them again, with what it costs.
    fn render_restyled(&self, view: &ScenesView, cx: &mut Context<Self>) -> Option<AnyElement> {
        if view.restyled.is_empty() {
            return None;
        }
        let bardo = self.bardo.read(cx);
        let busy = view.is_busy() || view.is_animating();
        let style = bardo.text(Text::VisualStyleName(view.style));
        let estimate = view
            .restyle_estimate
            .as_ref()
            .filter(|_| !busy)
            .and_then(|estimate| estimate_note(bardo, estimate, Text::EstimateCost, cx));
        let card = kit::card(cx)
            .px_3()
            .py_2()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap_2()
            .items_center()
            .border_color(cx.theme().warning)
            .child(
                h_flex()
                    .flex_1()
                    .gap_1()
                    .items_center()
                    .child(
                        div()
                            .font_medium()
                            .child(SharedString::from(bardo.text_with(
                                Text::ScenesRestyled,
                                &[("n", &view.restyled.len().to_string())],
                            ))),
                    )
                    .child(guide::info(
                        bardo,
                        "scenes-restyled-info",
                        SharedString::from(
                            bardo.text_with(Text::ScenesRestyledHint, &[("style", &style)]),
                        ),
                        guide::refs::SCENES_STYLE,
                    )),
            )
            .children(estimate)
            .when(!busy, |card| {
                card.child(
                    self.paid(
                        Button::new("redraw-restyled-scenes")
                            .outline()
                            .small()
                            .label(tr(bardo, Text::RedrawRestyled))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.run_scene(
                                    SceneAction::RedrawRestyled,
                                    BudgetConsent::Ask,
                                    window,
                                    cx,
                                )
                            })),
                        cx,
                    ),
                )
            });
        Some(card.into_any_element())
    }

    /// What happened to the scenes: an error, the job, why nothing can be
    /// planned yet, and the questions before discarding images or going
    /// past a budget.
    fn scenes_notices(
        &self,
        stage: Stage,
        view: &ScenesView,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let job = view
            .job
            .as_ref()
            .and_then(|job| self.render_scenes_job(job, cx));
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        // A cancelled job reads cancelled at once, and the stage stays busy
        // until its handler lets go of what it had sent: say why.
        let stopping = view.stopping.then(|| {
            h_flex()
                .gap_2()
                .items_center()
                .child(Spinner::new().small())
                .child(div().text_sm().child(tr(bardo, Text::ScenesStopping)))
                .into_any_element()
        });
        let files = view.plan.as_ref().map_or(0, |plan| plan.files().count());
        let confirm = self.confirm_replan.then(|| {
            kit::card(cx)
                .p_3()
                .gap_2()
                .border_color(theme.warning)
                .child(div().text_sm().child(SharedString::from(
                    bardo.text_with(Text::ReplanScenesConfirm, &[("n", &files.to_string())]),
                )))
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("confirm-replan-scenes")
                                .danger()
                                .small()
                                .label(tr(bardo, Text::ConfirmReplanScenes))
                                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                    this.run_scene(
                                        SceneAction::Plan {
                                            discard_images: true,
                                        },
                                        BudgetConsent::Ask,
                                        window,
                                        cx,
                                    )
                                })),
                        )
                        .child(
                            Button::new("cancel-replan-scenes")
                                .ghost()
                                .small()
                                .label(tr(bardo, Text::CancelReplanScenes))
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.confirm_replan = false;
                                    cx.notify();
                                })),
                        ),
                )
                .into_any_element()
        });
        let ask = self.scenes_ask.as_ref().map(|(action, estimate)| {
            let action = *action;
            budget_question(
                "scenes-budget",
                bardo,
                estimate,
                cx,
                cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.run_scene(action, BudgetConsent::Confirmed, window, cx)
                }),
                cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.scenes_ask = None;
                    cx.notify();
                }),
            )
        });
        let no_narration = view.narration.is_none().then(|| {
            kit::notice(Tone::Info, tr(bardo, Text::ScenesNoNarration), cx).into_any_element()
        });
        // Where the plan is asked for: the clips of a plan already made
        // do not wait for another.
        let too_long = (view.too_long && stage == Stage::Scenes).then(|| {
            kit::notice(Tone::Warning, tr(bardo, Text::ScenesNarrationTooLong), cx)
                .into_any_element()
        });
        let place = self.render_place_scenes(view, cx);
        let fit = self.render_fit_scenes(view, cx);
        let restyled = self.render_restyled(view, cx);
        let bardo = self.bardo.read(cx);
        self.scenes_error
            .map(|error| kit::notice(Tone::Danger, tr(bardo, error), cx).into_any_element())
            .into_iter()
            .chain(job)
            .chain(stopping)
            .chain(no_narration)
            .chain(too_long)
            .chain(place)
            .chain(fit)
            .chain(restyled)
            .chain(confirm)
            .chain(ask)
            .collect()
    }

    /// The scenes, every one or only those with something left: their
    /// picture, time and narration, prompt, image and clip states, clip
    /// model and price; ↑/↓ and Enter over them.
    fn scene_grid(
        &self,
        stage: Stage,
        view: &ScenesView,
        states: &[SceneState],
        selected: Option<usize>,
        cx: &mut Context<Self>,
    ) -> Collection {
        let keys = self.scene_keys(stage, selected.map(|index| states[index]), cx);
        let bardo = self.bardo.read(cx);
        let t = look(cx).tokens;
        let mut collection = Collection::new(CollectionKind::Grid, "scene-grid");
        let scenes = view.plan.as_ref().map_or(&[][..], |plan| plan.scenes());
        // What a scene has, shown by its icon; what it lacks is its chip's
        // to say, so no mark tells by color alone.
        let mark = |icon: Lucide, present: bool| {
            present.then(|| {
                Icon::new(icon)
                    .size(px(14.))
                    .text_color(t.text2)
                    .into_any_element()
            })
        };
        let image_states = scene_states(view, Stage::Scenes);
        let clip_states = scene_states(view, Stage::Clips);
        collection.headings = Headings {
            picture: Some(tr(bardo, Text::SceneColumnPicture)),
            time: Some(tr(bardo, Text::SceneColumnTime)),
            text: Some(tr(bardo, Text::SceneColumnNarration)),
            detail: Some(tr(bardo, Text::SceneColumnPrompt)),
        };
        collection.keys = Some(keys);
        collection.tiles = scenes
            .iter()
            .enumerate()
            .filter(|(index, _)| !self.pending_only || states[*index].is_pending())
            .map(|(index, scene)| {
                let state = states[index];
                let clip = view.clips.scenes.get(index);
                let plain = |text: SharedString| div().truncate().child(text).into_any_element();
                let dash = || plain(SharedString::from("—"));
                let mut tile = Tile::new(
                    ("scene-card", index),
                    Rc::new(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.selected_scene = Some(index);
                        cx.notify();
                    })),
                );
                tile.selected = selected == Some(index);
                tile.picture = scene.image().map(|image| {
                    img(bardo.scene_image_path(image))
                        .size_full()
                        .object_fit(ObjectFit::Cover)
                        .into_any_element()
                });
                tile.number = Some(SharedString::from((index + 1).to_string()));
                tile.time = Some(SharedString::from(format!(
                    "{}–{}",
                    clock(scene.start),
                    clock(scene.end)
                )));
                tile.text = Some(SharedString::from(scene.text.clone()));
                tile.status = scene_chip(bardo, state, cx);
                tile.marks = mark(Lucide::Image, scene.image().is_some())
                    .into_iter()
                    .chain(mark(Lucide::Film, scene.clip().is_some()))
                    .collect();
                tile.attention = state == SceneState::Review;
                tile.failed = state == SceneState::Failed;
                tile.detail = Some(SharedString::from(scene.prompt().as_str().to_owned()));
                tile.facts = vec![
                    Fact {
                        label: tr(bardo, Text::SceneColumnImage),
                        value: state_chip(bardo, image_states[index], cx),
                        numeric: false,
                    },
                    Fact {
                        label: tr(bardo, Text::SceneColumnClip),
                        value: match clip_states[index] {
                            // Nothing to animate yet: the image column says why.
                            SceneState::ToDraw => dash(),
                            clip_state => state_chip(bardo, clip_state, cx),
                        },
                        numeric: false,
                    },
                    Fact {
                        label: tr(bardo, Text::SceneColumnModel),
                        value: clip
                            .and_then(|clip| clip.model.as_ref())
                            .map_or_else(dash, |model| {
                                plain(SharedString::from(model.name.clone()))
                            }),
                        numeric: false,
                    },
                    Fact {
                        label: tr(bardo, Text::SceneColumnCost),
                        value: clip.and_then(|clip| clip.price).map_or_else(dash, |price| {
                            plain(SharedString::from(bardo.money(price)))
                        }),
                        numeric: true,
                    },
                ];
                tile
            })
            .collect();
        let empty = if scenes.is_empty() {
            Text::ScenesEmpty
        } else {
            Text::FilterPendingEmpty
        };
        collection.empty = Some(muted(cx, tr(bardo, empty)));
        collection
    }

    /// ↑/↓ walk the scenes; Enter accepts what the selected one waits on.
    fn scene_keys(
        &self,
        stage: Stage,
        selected: Option<SceneState>,
        cx: &mut Context<Self>,
    ) -> CollectionKeys {
        let screen = cx.entity().downgrade();
        let on_step = Rc::new(move |step: Step, _: &mut Window, cx: &mut App| {
            let _ = screen.update(cx, |this, cx| this.step_scene(stage, step, cx));
        });
        let screen = cx.entity().downgrade();
        let on_enter = (selected == Some(SceneState::Review)).then(|| {
            Rc::new(move |window: &mut Window, cx: &mut App| {
                let _ = screen.update(cx, |this, cx| this.accept_shown(stage, window, cx));
            }) as crate::parts::OnKey
        });
        CollectionKeys {
            focus: self.scene_keys.clone(),
            on_step,
            on_enter,
            hint: Some(guide::keys_hint(
                self.bardo.read(cx),
                Text::SceneKeysHint,
                guide::refs::SHORTCUTS_LISTS,
            )),
            scroll: self.scene_scroll.clone(),
        }
    }

    /// The picked scene: its narration, then its image and prompt (Scenes)
    /// or how it moves and its clip (Clips); provenance behind "Generation
    /// details".
    fn scene_inspector(&self, stage: Stage, index: usize, cx: &mut Context<Self>) -> Inspector {
        let Some(view) = self.scenes.as_ref() else {
            return Inspector::new(Vec::new());
        };
        let Some(scene) = view.plan.as_ref().and_then(|plan| plan.scenes().get(index)) else {
            return Inspector::new(Vec::new());
        };
        let open = self
            .editing_scene
            .filter(|(_, editing, _)| *editing == index)
            .map(|(_, _, field)| field);
        let busy = view.is_busy();
        // The body and where in it the picture is.
        let (stage_body, media): (Vec<AnyElement>, usize) = if stage == Stage::Clips {
            let clip = view
                .clips
                .scenes
                .get(index)
                .filter(|_| scene.can_animate())
                .map(|clip| self.render_scene_clip(index, scene, clip, open, cx));
            let bardo = self.bardo.read(cx);
            (
                std::iter::once(scene_picture(bardo, scene.image(), cx))
                    .chain(clip.or_else(|| Some(muted(cx, tr(bardo, Text::SceneNoImage)))))
                    .collect(),
                0,
            )
        } else {
            self.scene_image_body(index, scene, open, busy, cx)
        };
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();

        let title = h_flex()
            .gap_2()
            .items_center()
            .child(
                div()
                    .text_sm()
                    .font_semibold()
                    .child(SharedString::from(bardo.text_with(
                        Text::SceneLabel,
                        &[
                            ("n", &(index + 1).to_string()),
                            ("start", &clock(scene.start)),
                            ("end", &clock(scene.end)),
                        ],
                    ))),
            )
            .when(scene.is_edited(), |row| {
                row.child(kit::status(Tone::Info, tr(bardo, Text::SceneEdited), cx))
            })
            .into_any_element();
        let narration = v_flex()
            .gap_1()
            .child(field_label(tr(bardo, Text::NarrationTitle)))
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(SharedString::from(scene.text.clone())),
            )
            .into_any_element();

        // Provenance: the image and the clip.
        let models = &view.clips.models;
        let lines: Vec<SharedString> = scene
            .image()
            .map(|image| {
                let generation = &image.generation;
                let usage = generation.usage;
                SharedString::from(bardo.text_with(
                    Text::SceneImageRecord,
                    &[
                        ("model", &generation.model),
                        (
                            "tokens",
                            &(usage.input_tokens + usage.output_tokens).to_string(),
                        ),
                        ("n", &generation.template.number.to_string()),
                    ],
                ))
            })
            .into_iter()
            .chain(scene.clip().map(|clip| clip_record(bardo, models, clip)))
            .collect();
        let footer = (!lines.is_empty()).then(|| {
            kit::details(
                ("scene-details", index),
                tr(bardo, Text::GenerationDetails),
                lines,
            )
            .into_any_element()
        });

        let mut inspector = Inspector::new(std::iter::once(narration).chain(stage_body).collect());
        // After the narration.
        inspector.media = Some(1 + media);
        inspector.title = Some(title);
        inspector.footer = footer;
        inspector.scroll = Some(self.inspector_scroll.clone());
        inspector
    }

    /// The Scenes stage of the inspector: why the image failed, the image
    /// (or the current one beside a new one to review), its prompt, and
    /// drawing it again; with where the image is.
    fn scene_image_body(
        &self,
        index: usize,
        scene: &Scene,
        open: Option<ScenePromptField>,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> (Vec<AnyElement>, usize) {
        let editor = (open == Some(ScenePromptField::Image))
            .then(|| self.render_prompt_editor(index, None, cx));
        let bardo = self.bardo.read(cx);
        let redraw_estimate = self
            .scenes
            .as_ref()
            .and_then(|view| view.image_estimates.get(index));

        let failure = scene.failure().map(|kind| {
            kit::notice(
                Tone::Danger,
                format!(
                    "{} {}",
                    bardo.text(Text::SceneFailed),
                    bardo.text(Text::JobFailureKindName(kind))
                ),
                cx,
            )
            .text_xs()
            .into_any_element()
        });

        let picture = match scene.pending() {
            None => scene_picture(bardo, scene.image(), cx),
            // The new image to review is where drawing again leads.
            Some(pending) => kit::anchor(
                TourAnchor::Control(Control::SceneRedraw),
                v_flex()
                    .gap_2()
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(
                                div()
                                    .text_sm()
                                    .font_medium()
                                    .child(tr(bardo, Text::ScenePendingTitle)),
                            )
                            .child(guide::info(
                                bardo,
                                ("pending-image-info", index),
                                tr(bardo, Text::ScenePendingHint),
                                guide::refs::SCENES_REDRAW,
                            )),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap_1()
                                    .child(field_label(tr(bardo, Text::SceneCurrentImage)))
                                    .child(scene_picture(bardo, scene.image(), cx)),
                            )
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap_1()
                                    .child(field_label(tr(bardo, Text::SceneNewImage)))
                                    .child(scene_picture(bardo, Some(pending), cx)),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new(("accept-scene-image", index))
                                    .primary()
                                    .xsmall()
                                    .label(tr(bardo, Text::AcceptSceneImage))
                                    .on_click(cx.listener(
                                        move |this, _: &ClickEvent, window, cx| {
                                            this.scene_action(window, cx, |bardo, id| {
                                                bardo.accept_scene_image(id, index)
                                            });
                                        },
                                    )),
                            )
                            .child(
                                Button::new(("reject-scene-image", index))
                                    .ghost()
                                    .xsmall()
                                    .label(tr(bardo, Text::RejectSceneImage))
                                    .on_click(cx.listener(
                                        move |this, _: &ClickEvent, window, cx| {
                                            this.scene_action(window, cx, |bardo, id| {
                                                bardo.reject_scene_image(id, index)
                                            });
                                        },
                                    )),
                            ),
                    ),
            )
            .into_any_element(),
        };

        let prompt_label = h_flex()
            .gap_2()
            .items_center()
            .child(field_label(tr(bardo, Text::SceneImagePrompt)))
            .child(div().flex_1())
            // Drawing while the prompt is open would draw the saved
            // prompt, not the one being typed, so the actions wait for the
            // editor to close.
            .when(open.is_none(), |row| {
                row.child(
                    Button::new(("edit-scene-prompt", index))
                        .ghost()
                        .xsmall()
                        .label(tr(bardo, Text::EditScenePrompt))
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.edit_scene(index, ScenePromptField::Image, window, cx)
                        })),
                )
            })
            .into_any_element();
        let prompt: AnyElement = match editor {
            Some(editor) => editor,
            None => kit::well(cx)
                .text_xs()
                .child(SharedString::from(scene.prompt().as_str().to_owned()))
                .into_any_element(),
        };
        let redraw = (open.is_none()
            && scene.image().is_some()
            && !busy
            && scene.pending().is_none())
        .then(|| {
            kit::anchor_in(
                TourAnchor::Control(Control::SceneRedraw),
                v_flex()
                    .gap_1()
                    .children(redraw_estimate.and_then(|estimate| {
                        estimate_note(bardo, estimate, Text::EstimateRedraw, cx)
                    }))
                    .child(
                        h_flex().child(
                            self.paid(Button::new(("regenerate-scene-image", index)), cx)
                                .outline()
                                .small()
                                .label(tr(bardo, Text::RegenerateSceneImage))
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    this.run_scene(
                                        SceneAction::Redraw(index),
                                        BudgetConsent::Ask,
                                        window,
                                        cx,
                                    );
                                })),
                        ),
                    ),
                Some(&self.inspector_scroll),
            )
            .into_any_element()
        });

        let media = usize::from(failure.is_some());
        let place = self.render_scene_place(index, scene, open, busy, cx);
        let cast = self.render_scene_cast(index, scene, open, busy, cx);
        let body = failure
            .into_iter()
            .chain(Some(picture))
            .chain(Some(
                v_flex()
                    .gap_1()
                    .child(prompt_label)
                    .child(prompt)
                    .into_any_element(),
            ))
            .chain(redraw)
            .chain(place)
            .chain(cast)
            .collect();
        (body, media)
    }

    /// The recurring place scene `index` happens in (#258): its name and
    /// the one description every scene there is drawn with, with editing
    /// it. `None` for a scene in a place of its own.
    fn render_scene_place(
        &self,
        index: usize,
        scene: &Scene,
        open: Option<ScenePromptField>,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let plan = self.scenes.as_ref()?.plan.as_ref()?;
        let at = scene.place()?;
        let place = plan.place(at).ok()?.clone();
        // An edited prompt that stopped naming the place left it.
        let left = plan.scene_place(index).is_none();
        let editor = (open == Some(ScenePromptField::Place(at)))
            .then(|| self.render_prompt_editor(index, None, cx));
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let title = h_flex()
            .gap_1()
            .items_center()
            .child(field_label(tr(bardo, Text::ScenePlace)))
            .child(guide::info(
                bardo,
                ("scene-place-info", index),
                tr(bardo, Text::ScenePlaceHint),
                guide::refs::SCENES_SAME_PLACE,
            ))
            .child(div().flex_1())
            .when(open.is_none() && !busy, |row| {
                row.child(
                    Button::new(("edit-place", index))
                        .ghost()
                        .xsmall()
                        .label(tr(bardo, Text::EditPlaceDescription))
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.edit_scene(index, ScenePromptField::Place(at), window, cx)
                        })),
                )
            });
        let name = h_flex()
            .gap_2()
            .items_center()
            .child(
                div()
                    .text_sm()
                    .font_semibold()
                    .child(SharedString::from(place.name.clone())),
            )
            .when(place.is_edited(), |row| {
                row.child(kit::status(Tone::Info, tr(bardo, Text::PlaceEdited), cx))
            });
        let description: AnyElement = match editor {
            Some(editor) => editor,
            None => kit::well(cx)
                .text_xs()
                .child(SharedString::from(place.description().as_str().to_owned()))
                .into_any_element(),
        };
        let left = left.then(|| {
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(tr(bardo, Text::ScenePlaceLeft))
        });
        Some(
            v_flex()
                .gap_1()
                .child(title)
                .child(name)
                .child(description)
                .children(left)
                .into_any_element(),
        )
    }

    /// The recurring people scene `index` shows (#246): each one's
    /// portrait, name and look, with editing the look and drawing the
    /// portrait again. `None` for a scene that shows nobody who recurs.
    fn render_scene_cast(
        &self,
        index: usize,
        scene: &Scene,
        open: Option<ScenePromptField>,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let plan = self.scenes.as_ref()?.plan.as_ref()?;
        if scene.cast().is_empty() {
            return None;
        }
        let members: Vec<(usize, CastMember)> = scene
            .cast()
            .iter()
            .filter_map(|&member| Some((member, plan.cast_member(member).ok()?.clone())))
            .collect();
        let editors: Vec<Option<AnyElement>> = members
            .iter()
            .map(|(member, _)| {
                (open == Some(ScenePromptField::Cast(*member)))
                    .then(|| self.render_prompt_editor(index, None, cx))
            })
            .collect();
        let bardo = self.bardo.read(cx);
        let t = look(cx).tokens;
        let theme = cx.theme();
        let rows = members
            .into_iter()
            .zip(editors)
            .map(|((member, cast), editor)| {
                let portrait = div()
                    .w(px(56.))
                    .h(px(84.))
                    .flex_none()
                    .rounded(t.radius)
                    .overflow_hidden()
                    .bg(t.sunken);
                let portrait = match cast.portrait() {
                    Some(image) => portrait
                        .child(
                            img(bardo.scene_image_path(image))
                                .size_full()
                                .object_fit(ObjectFit::Cover),
                        )
                        .into_any_element(),
                    None => portrait.into_any_element(),
                };
                let name = h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .text_sm()
                            .font_semibold()
                            .child(SharedString::from(cast.name.clone())),
                    )
                    .when(cast.is_edited(), |row| {
                        row.child(kit::status(Tone::Info, tr(bardo, Text::CastEdited), cx))
                    });
                let description: AnyElement = match editor {
                    Some(editor) => editor,
                    None => kit::well(cx)
                        .text_xs()
                        .child(SharedString::from(cast.description().as_str().to_owned()))
                        .into_any_element(),
                };
                let failure = cast.failure().map(|kind| {
                    kit::notice(
                        Tone::Danger,
                        format!(
                            "{} {}",
                            bardo.text(Text::CastPortraitFailed),
                            bardo.text(Text::JobFailureKindName(kind))
                        ),
                        cx,
                    )
                    .text_xs()
                });
                let later = cast.portrait().is_none().then(|| {
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(tr(bardo, Text::CastPortraitLater))
                });
                let actions = (open.is_none() && !busy).then(|| {
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .child(
                            Button::new(("edit-cast", member))
                                .ghost()
                                .xsmall()
                                .label(tr(bardo, Text::EditCastDescription))
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    this.edit_scene(
                                        index,
                                        ScenePromptField::Cast(member),
                                        window,
                                        cx,
                                    )
                                })),
                        )
                        .when(
                            cast.portrait().is_some() || cast.failure().is_some(),
                            |row| {
                                row.child(
                                    self.paid(Button::new(("redraw-portrait", member)), cx)
                                        .outline()
                                        .xsmall()
                                        .label(tr(bardo, Text::RedrawCastPortrait))
                                        .on_click(cx.listener(
                                            move |this, _: &ClickEvent, window, cx| {
                                                this.run_scene(
                                                    SceneAction::RedrawPortrait(member),
                                                    BudgetConsent::Ask,
                                                    window,
                                                    cx,
                                                );
                                            },
                                        )),
                                )
                            },
                        )
                });
                h_flex()
                    .gap_2()
                    .items_start()
                    .child(portrait)
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(name)
                            .child(description)
                            .children(failure)
                            .children(later)
                            .children(actions),
                    )
                    .into_any_element()
            });
        let title = h_flex()
            .gap_1()
            .items_center()
            .child(field_label(tr(bardo, Text::SceneCast)))
            .child(guide::info(
                bardo,
                ("scene-cast-info", index),
                tr(bardo, Text::SceneCastHint),
                guide::refs::SCENES_CAST,
            ));
        Some(
            v_flex()
                .gap_2()
                .child(title)
                .children(rows)
                .into_any_element(),
        )
    }

    /// A plan or image job running, or why the last one stopped.
    fn render_scenes_job(&self, job: &Job, cx: &App) -> Option<AnyElement> {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let running = if job.kind() == bardo_app::bardo_domain::JobKind::ScenePlan {
            Text::ScenesPlanning
        } else {
            Text::ScenesDrawing
        };
        match job.state() {
            JobState::Queued | JobState::Running => Some(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(Spinner::new().small())
                    .child(div().text_sm().child(tr(bardo, running)))
                    .child(div().text_xs().text_color(theme.muted_foreground).child(
                        SharedString::from(format!("{}%", job.progress().permille() / 10)),
                    ))
                    .into_any_element(),
            ),
            JobState::Failed => {
                let failure = job.failure()?;
                Some(
                    v_flex()
                        .gap_1()
                        .child(kit::notice(
                            Tone::Danger,
                            tr(bardo, Text::ScenesStopped),
                            cx,
                        ))
                        .child(
                            h_flex()
                                .gap_1()
                                .child(
                                    div()
                                        .text_xs()
                                        .child(tr(bardo, Text::JobFailureKindName(failure.kind))),
                                )
                                .child(kit::details(
                                    "scenes-failure-details",
                                    tr(bardo, Text::Details),
                                    vec![SharedString::from(failure.detail.clone())],
                                )),
                        )
                        .into_any_element(),
                )
            }
            JobState::Cancelled | JobState::Done => None,
        }
    }

    /// The open prompt editor of scene `index`, with `hint` above its
    /// buttons.
    fn render_prompt_editor(
        &self,
        index: usize,
        hint: Option<Text>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        v_flex()
            .gap_1()
            .child(Textarea::new(&self.scene_editor))
            .children(self.scene_field_error.map(|error| {
                kit::notice(Tone::Danger, tr(bardo, Text::SceneFieldError(error)), cx).text_xs()
            }))
            .children(hint.map(|hint| {
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(tr(bardo, hint))
            }))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new(("save-scene-prompt", index))
                            .primary()
                            .xsmall()
                            .label(tr(bardo, Text::SaveScenePrompt))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.save_scene_prompt(window, cx)
                            })),
                    )
                    .child(
                        Button::new(("cancel-scene-prompt", index))
                            .ghost()
                            .xsmall()
                            .label(tr(bardo, Text::CancelScenePrompt))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.editing_scene = None;
                                this.scene_field_error = None;
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    /// Opens a clip in the system's video player.
    fn play_clip(&mut self, clip: &SceneClip, cx: &mut Context<Self>) {
        let path = self.bardo.read(cx).scene_clip_path(clip);
        cx.open_with_system(&path);
    }

    /// Scene `index`'s clip part: how it moves, which model animates it,
    /// the clip being made or why it failed, its clip and its new clip.
    fn render_scene_clip(
        &self,
        index: usize,
        scene: &Scene,
        view: &SceneClipView,
        open: Option<ScenePromptField>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let editing = open == Some(ScenePromptField::Motion);
        let motion: AnyElement = if editing {
            self.render_prompt_editor(index, Some(Text::MotionPromptHint), cx)
        } else {
            kit::well(cx)
                .text_xs()
                .child(SharedString::from(
                    scene.motion_prompt().as_str().to_owned(),
                ))
                .into_any_element()
        };
        let models = self
            .scenes
            .as_ref()
            .map(|scenes| scenes.clips.models.clone())
            .unwrap_or_default();
        let channel_model = self
            .scenes
            .as_ref()
            .and_then(|scenes| scenes.clips.default_model.clone());
        let screen = cx.entity().downgrade();
        let accent_edge = look(cx).tokens.accent_edge;
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();

        let motion_label = h_flex()
            .gap_2()
            .items_center()
            .child(
                div()
                    .text_xs()
                    .font_semibold()
                    .child(tr(bardo, Text::SceneMotionPrompt)),
            )
            .when(
                scene.own_motion_prompt().is_none() && scene.planned_motion().is_none(),
                |row| {
                    row.child(
                        Tag::secondary()
                            .small()
                            .child(tr(bardo, Text::SceneMotionFromImage)),
                    )
                },
            );

        // The model menu: the channel's model, then every model by name.
        let model_label = match (scene.clip_model(), &view.model) {
            (None, _) => SharedString::from(
                bardo.text_with(
                    Text::SceneClipModelChannel,
                    &[(
                        "model",
                        channel_model
                            .as_ref()
                            .map_or("", |model| model.name.as_str()),
                    )],
                ),
            ),
            (Some(_), Some(model)) => SharedString::from(model.name.clone()),
            (Some(own), None) => SharedString::from(own.model().to_owned()),
        };
        // A model its provider retired checks the one that took its place.
        let own = scene.clip_model().map(|model| bardo.clip_model_now(model));
        let channel_title = SharedString::from(
            bardo.text_with(
                Text::SceneClipModelChannel,
                &[(
                    "model",
                    channel_model
                        .as_ref()
                        .map_or("", |model| model.name.as_str()),
                )],
            ),
        );
        // Every model under its provider's name; with two providers the
        // list is longer than some windows, so it scrolls.
        let menu_models: Vec<(Option<SharedString>, ClipModel)> = models
            .iter()
            .enumerate()
            .map(|(at, model)| {
                let provider = model.id.provider();
                let starts_group = at == 0 || models[at - 1].id.provider() != provider;
                let heading = starts_group.then(|| tr(bardo, Text::ProviderName(provider)));
                (heading, model.clone())
            })
            .collect();
        let model_menu = Button::new(("scene-clip-model", index))
            .ghost()
            .xsmall()
            .dropdown_caret(true)
            .label(model_label.clone())
            .dropdown_menu(move |menu, _, _| {
                let pick = |value: Option<ClipModelRef>, title: SharedString| {
                    let screen = screen.clone();
                    let checked = value == own;
                    PopupMenuItem::new(title)
                        .checked(checked)
                        .on_click(move |_, window, cx| {
                            let value = value.clone();
                            let _ = screen.update(cx, |this, cx| {
                                this.scene_action(window, cx, |bardo, id| {
                                    bardo.set_scene_clip_model(id, index, value)
                                });
                            });
                        })
                };
                let mut menu = menu
                    .scrollable(true)
                    .item(pick(None, channel_title.clone()));
                for (heading, model) in &menu_models {
                    if let Some(heading) = heading {
                        menu = menu.separator().label(heading.clone());
                    }
                    menu = menu.item(pick(
                        Some(model.id.clone()),
                        SharedString::from(model.name.clone()),
                    ));
                }
                menu
            });
        let plan_note = view.seconds.map(|seconds| {
            let seconds = seconds.to_string();
            let text = match view.price {
                Some(price) => bardo.text_with(
                    Text::SceneClipPlan,
                    &[("seconds", &seconds), ("price", &bardo.money(price))],
                ),
                None => bardo.text_with(Text::SceneClipPlanUnpriced, &[("seconds", &seconds)]),
            };
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(SharedString::from(text))
        });
        // A clip asked for shorter than the scene says so on a line of its
        // own, with why behind the ⓘ.
        let slowed_note = (view.seconds.is_some() && view.stretched).then(|| {
            h_flex()
                .gap_1()
                .items_center()
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(tr(bardo, Text::SceneClipSlowedNext)),
                )
                .child(guide::info(
                    bardo,
                    ("next-clip-slowed-info", index),
                    tr(bardo, Text::SceneClipSlowedHint),
                    guide::refs::CLIPS_STRETCH,
                ))
        });
        // While a clip is being made its model can't change: the name
        // shows without the menu.
        let model_menu: AnyElement = if view.is_busy() {
            div().text_xs().px_2().child(model_label).into_any_element()
        } else {
            model_menu.into_any_element()
        };
        let model_gone = view
            .model
            .is_none()
            .then(|| kit::notice(Tone::Warning, tr(bardo, Text::SceneClipModelGone), cx).text_xs());
        // Its provider retired the scene's model: say which one runs.
        let model_replaced = view
            .replaces
            .as_ref()
            .zip(view.model.as_ref())
            .map(|(old, new)| {
                let text = bardo.text_with(
                    Text::SceneClipModelReplaced,
                    &[("old", old), ("new", &new.name)],
                );
                kit::notice(Tone::Info, SharedString::from(text), cx).text_xs()
            });

        let state: Option<AnyElement> = if view.is_busy() {
            Some(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(Spinner::new().small())
                    .child(div().text_xs().child(tr(bardo, Text::SceneAnimating)))
                    .into_any_element(),
            )
        } else {
            scene.clip_failure().map(|kind| {
                let detail = view
                    .job
                    .as_ref()
                    .filter(|job| job.state() == JobState::Failed)
                    .and_then(|job| job.failure())
                    .map(|failure| failure.detail.clone());
                h_flex()
                    .gap_1()
                    .child(
                        kit::notice(
                            Tone::Danger,
                            format!(
                                "{} {}",
                                bardo.text(Text::SceneClipFailed),
                                bardo.text(Text::JobFailureKindName(kind))
                            ),
                            cx,
                        )
                        .text_xs(),
                    )
                    .children(detail.map(|detail| {
                        kit::details(
                            ("scene-clip-failure-details", index),
                            tr(bardo, Text::Details),
                            vec![SharedString::from(detail)],
                        )
                    }))
                    .into_any_element()
            })
        };

        let actions = open.is_none().then(|| {
            h_flex()
                .gap_2()
                .child(
                    Button::new(("edit-motion-prompt", index))
                        .ghost()
                        .xsmall()
                        .label(tr(bardo, Text::EditMotionPrompt))
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.edit_scene(index, ScenePromptField::Motion, window, cx)
                        })),
                )
                .when(
                    !view.is_busy() && view.model.is_some() && scene.pending_clip().is_none(),
                    |row| {
                        row.child(
                            self.paid(Button::new(("animate-scene", index)), cx)
                                .outline()
                                .xsmall()
                                .label(tr(
                                    bardo,
                                    if scene.clip().is_some() {
                                        Text::AnimateSceneAgain
                                    } else {
                                        Text::AnimateScene
                                    },
                                ))
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    this.run_scene(
                                        SceneAction::Animate(index),
                                        BudgetConsent::Ask,
                                        window,
                                        cx,
                                    );
                                })),
                        )
                    },
                )
        });

        let record = |clip: &SceneClip| clip_record(bardo, &models, clip);
        // Tagged only while it plays slowed: a scene that shrank to the
        // clip's own length plays it at its own speed.
        let plays_slowed = |clip: &SceneClip| clip.played_over(scene.duration()) > clip.recorded();
        let current = scene.clip().map(|clip| {
            let played = clip.clone();
            h_flex()
                .gap_2()
                .flex_wrap()
                .items_center()
                .child(Tag::secondary().small().child(record(clip)))
                .when(plays_slowed(clip), |row| {
                    row.child(
                        Tag::secondary()
                            .small()
                            .child(tr(bardo, Text::SceneClipSlowed)),
                    )
                })
                .when(scene.is_clip_stale(), |row| {
                    row.child(kit::status(
                        Tone::Warning,
                        tr(bardo, Text::SceneClipStale),
                        cx,
                    ))
                })
                .child(
                    Button::new(("play-scene-clip", index))
                        .ghost()
                        .xsmall()
                        .label(tr(bardo, Text::PlayClip))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.play_clip(&played, cx)
                        })),
                )
                .when(!view.is_busy(), |row| {
                    row.child(
                        Button::new(("use-scene-still", index))
                            .ghost()
                            .xsmall()
                            .label(tr(bardo, Text::UseSceneStill))
                            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                this.scene_action(window, cx, |bardo, id| {
                                    bardo.use_scene_still(id, index)
                                });
                            })),
                    )
                })
        });

        let pending = scene.pending_clip().map(|clip| {
            let played = clip.clone();
            kit::card(cx)
                .p_2()
                .gap_1p5()
                .border_color(accent_edge)
                .child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .items_center()
                        .child(
                            div()
                                .text_sm()
                                .font_medium()
                                .child(tr(bardo, Text::ScenePendingClipTitle)),
                        )
                        .child(Tag::secondary().small().child(record(clip)))
                        .when(plays_slowed(clip), |row| {
                            row.child(
                                Tag::secondary()
                                    .small()
                                    .child(tr(bardo, Text::SceneClipSlowed)),
                            )
                        })
                        .child(guide::info(
                            bardo,
                            ("pending-clip-info", index),
                            tr(bardo, Text::ScenePendingClipHint),
                            guide::refs::CLIPS_REVIEW,
                        )),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new(("play-pending-clip", index))
                                .outline()
                                .xsmall()
                                .label(tr(bardo, Text::PlayClip))
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.play_clip(&played, cx)
                                })),
                        )
                        .child(
                            Button::new(("accept-scene-clip", index))
                                .primary()
                                .xsmall()
                                .label(tr(bardo, Text::AcceptSceneClip))
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    this.scene_action(window, cx, |bardo, id| {
                                        bardo.accept_scene_clip(id, index)
                                    });
                                })),
                        )
                        .child(
                            Button::new(("reject-scene-clip", index))
                                .ghost()
                                .xsmall()
                                .label(tr(bardo, Text::RejectSceneClip))
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    this.scene_action(window, cx, |bardo, id| {
                                        bardo.reject_scene_clip(id, index)
                                    });
                                })),
                        ),
                )
        });

        let scroll = Some(&self.inspector_scroll);
        let review = (current.is_some() || pending.is_some()).then(|| {
            kit::anchor_in(
                TourAnchor::Control(Control::ClipReview),
                v_flex().gap_1p5().children(current).children(pending),
                scroll,
            )
        });
        v_flex()
            .gap_1p5()
            .child(kit::anchor_in(
                TourAnchor::Control(Control::ClipMotion),
                v_flex().gap_1p5().child(motion_label).child(motion),
                scroll,
            ))
            .child(
                h_flex()
                    .gap_x_2()
                    .flex_wrap()
                    .items_center()
                    .child(kit::anchor_in(
                        TourAnchor::Control(Control::ClipModel),
                        h_flex()
                            .gap_x_2()
                            .items_center()
                            .child(
                                div()
                                    .text_xs()
                                    .font_semibold()
                                    .child(tr(bardo, Text::SceneClipModel)),
                            )
                            .child(model_menu),
                        scroll,
                    ))
                    .children(plan_note.map(|note| {
                        kit::anchor_in(TourAnchor::Control(Control::ClipCost), note, scroll)
                    })),
            )
            .children(slowed_note)
            .children(model_gone)
            .children(model_replaced)
            .children(state)
            .children(actions)
            .children(review)
            .into_any_element()
    }
}

/// A scene's image filling a 16:9 box, or the box saying there is none.
fn scene_picture(bardo: &Bardo, image: Option<&SceneImage>, cx: &App) -> AnyElement {
    let t = look(cx).tokens;
    let frame = div()
        .w_full()
        .aspect_ratio(16. / 9.)
        .rounded(t.radius)
        .overflow_hidden()
        .bg(t.sunken);
    match image {
        Some(image) => frame
            .child(
                img(bardo.scene_image_path(image))
                    .size_full()
                    .object_fit(ObjectFit::Cover),
            )
            .into_any_element(),
        None => frame
            .flex()
            .items_center()
            .justify_center()
            .p_2()
            .text_xs()
            .text_color(t.text2)
            .child(tr(bardo, Text::SceneNoImage))
            .into_any_element(),
    }
}

/// A scene card's state chip; a scene with nothing left shows none.
/// A scene's state at a stage as a chip, "Ready" when nothing is left.
fn state_chip(bardo: &Bardo, state: SceneState, cx: &App) -> AnyElement {
    scene_chip(bardo, state, cx).unwrap_or_else(|| {
        kit::status(Tone::Success, tr(bardo, Text::SceneStateDone), cx).into_any_element()
    })
}

fn scene_chip(bardo: &Bardo, state: SceneState, cx: &App) -> Option<AnyElement> {
    let (tone, icon, text): (Tone, Icon, Text) = match state {
        SceneState::Done => return None,
        SceneState::ToDraw => (
            Tone::Neutral,
            Icon::new(Lucide::Image),
            Text::SceneCardToDraw,
        ),
        SceneState::ToAnimate => (
            Tone::Neutral,
            Icon::new(Lucide::Film),
            Text::SceneCardToAnimate,
        ),
        SceneState::Animating => (
            Tone::Info,
            Icon::new(IconName::LoaderCircle),
            Text::SceneCardAnimating,
        ),
        SceneState::Review => (
            Tone::Accent,
            Icon::new(IconName::Eye),
            Text::SceneCardReview,
        ),
        SceneState::Failed => (
            Tone::Danger,
            Icon::new(Tone::Danger.icon()),
            Text::SceneCardFailed,
        ),
    };
    Some(kit::status_with(tone, icon, tr(bardo, text), cx).into_any_element())
}

/// A small label over a field of the inspector.
fn field_label(text: SharedString) -> gpui_kit::Div {
    div().text_xs().font_semibold().child(text)
}

/// A clip's length and model, the model named as people call it while it
/// is still offered.
fn clip_record(bardo: &Bardo, models: &[ClipModel], clip: &SceneClip) -> SharedString {
    let model = models
        .iter()
        .find(|model| model.id.model() == clip.generation.model)
        .map_or(clip.generation.model.as_str(), |model| model.name.as_str());
    SharedString::from(bardo.text_with(
        Text::SceneClipRecord,
        &[("seconds", &clip.seconds.to_string()), ("model", model)],
    ))
}
