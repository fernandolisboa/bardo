//! The scenes panel of the projects screen: plan the narration's scenes,
//! read and edit each scene's image prompt, draw the images, draw one again
//! and choose between the current image and the new one. Then animate the
//! images into clips: pick each scene's video model, edit how it moves,
//! and play, keep or discard each new clip. Clips play in the system's
//! video player until the editor has its own.

use bardo_app::bardo_domain::{
    ClipModel, ClipModelRef, Job, JobState, Scene, SceneClip, SceneImage, TemplateKind,
    VideoProjectId,
};
use bardo_app::{BudgetConsent, SceneClipView, SceneError, ScenesView, Text};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Textarea;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, ClickEvent, ObjectFit, SharedString, Window, div, img, px};

use super::{ProjectsScreen, PromptShown, clock, muted};
use crate::appearance::look;
use crate::kit::{self, Tone};
use crate::shell::tr;
use crate::spend::{budget_question, estimate_note};

/// Thumbnails are 16:9, like the images.
const THUMB_WIDTH: f32 = 192.;
const THUMB_HEIGHT: f32 = 108.;

/// A scene action that starts a paid job.
#[derive(Clone, Copy)]
pub(super) enum SceneAction {
    Plan { discard_images: bool },
    DrawMissing,
    Redraw(usize),
    AnimateMissing,
    Animate(usize),
}

/// Which prompt of a scene the editor holds.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ScenePromptField {
    Image,
    Motion,
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
        // A motion prompt the scene does not have opens empty: saving it
        // empty keeps following the image prompt.
        let text = match field {
            ScenePromptField::Image => scene.prompt().as_str().to_owned(),
            ScenePromptField::Motion => scene
                .own_motion_prompt()
                .map(|prompt| prompt.as_str().to_owned())
                .unwrap_or_default(),
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

    /// The scenes: plan them, edit their prompts, draw their images, and
    /// review a new image of one scene.
    pub(super) fn render_scenes(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let view = self.scenes.as_ref()?;
        let job = view
            .job
            .as_ref()
            .and_then(|job| self.render_scenes_job(job, cx));
        let busy = view.is_busy();
        let plan = view.plan.as_ref();
        let cards: Vec<AnyElement> = plan
            .map(|plan| {
                plan.scenes()
                    .iter()
                    .enumerate()
                    .map(|(index, scene)| {
                        self.render_scene(index, scene, view.clips.scenes.get(index), busy, cx)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let animate = plan
            .filter(|plan| plan.scenes().iter().any(Scene::can_animate))
            .map(|_| self.render_animate_all(view, cx));
        let provenance = plan.map(|plan| {
            self.render_provenance(
                &plan.generation,
                TemplateKind::ImagePrompt,
                PromptShown::ScenePlan,
                cx,
            )
        });
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();

        let title_row = h_flex()
            .gap_2()
            .items_center()
            .child(kit::section_heading(tr(bardo, Text::ScenesTitle)))
            .children(plan.map(|plan| {
                Tag::secondary()
                    .small()
                    .child(SharedString::from(bardo.text_with(
                        Text::ScenesCount,
                        &[("n", &plan.scenes().len().to_string())],
                    )))
            }))
            .when(view.stale, |row| {
                row.child(kit::status(
                    Tone::Warning,
                    tr(bardo, Text::ScenesStaleTag),
                    cx,
                ))
                .child(kit::info(
                    "scenes-stale-info",
                    None,
                    tr(bardo, Text::ScenesStale),
                ))
            });

        let plan_button = Button::new("plan-scenes")
            .small()
            .label(tr(
                bardo,
                if plan.is_some() {
                    Text::ReplanScenes
                } else {
                    Text::PlanScenes
                },
            ))
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                this.run_scene(
                    SceneAction::Plan {
                        discard_images: false,
                    },
                    BudgetConsent::Ask,
                    window,
                    cx,
                )
            }));
        // Only the leading button is primary; drawing the missing images
        // leads once a plan exists.
        let missing = view.missing_images();
        let plan_button = if plan.is_none() || view.stale {
            plan_button.primary()
        } else {
            plan_button.outline()
        };
        let can_plan = !busy && !view.is_animating() && view.narration.is_some();
        let plan_button = can_plan.then_some(plan_button);
        let draw_button = (missing > 0 && !busy && !view.stale).then(|| {
            Button::new("generate-scene-images")
                .primary()
                .small()
                .label(SharedString::from(bardo.text_with(
                    Text::GenerateSceneImages,
                    &[("n", &missing.to_string())],
                )))
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.run_scene(SceneAction::DrawMissing, BudgetConsent::Ask, window, cx);
                }))
        });
        let files = plan.map_or(0, |plan| plan.files().count());
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
        });
        let no_narration = view.narration.is_none();
        let hint = match (view.narration, plan) {
            (None, _) => tr(bardo, Text::ScenesNoNarration),
            (Some(_), None) => SharedString::from(bardo.text_with(
                Text::PlanScenesHint,
                &[("n", &view.template.number.to_string())],
            )),
            (Some(_), Some(_)) => tr(bardo, Text::SceneImagesHint),
        };
        // The estimate of the button that leads: planning, drawing the
        // missing images, or drawing one scene again.
        let estimate = if plan.is_none() || view.stale {
            view.plan_estimate
                .as_ref()
                .and_then(|estimate| estimate_note(bardo, estimate, Text::EstimateCost, cx))
        } else if missing > 0 {
            view.images_estimate
                .as_ref()
                .and_then(|estimate| estimate_note(bardo, estimate, Text::EstimateCost, cx))
        } else {
            view.image_estimate
                .as_ref()
                .and_then(|estimate| estimate_note(bardo, estimate, Text::EstimateRedraw, cx))
        };
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

        Some(
            v_flex()
                .pt_3()
                .gap_2()
                .border_t_1()
                .border_color(theme.border)
                .child(title_row)
                .children(
                    self.scenes_error
                        .map(|error| kit::notice(Tone::Danger, tr(bardo, error), cx)),
                )
                .children(job)
                .when(plan.is_none(), |panel| {
                    panel.child(muted(cx, tr(bardo, Text::ScenesEmpty)))
                })
                .when(no_narration, |panel| {
                    panel.child(kit::notice(Tone::Info, hint.clone(), cx))
                })
                .when(draw_button.is_some() || plan_button.is_some(), |panel| {
                    panel.child(
                        v_flex().gap_1().children(estimate).child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .children(draw_button)
                                .children(plan_button)
                                .child(kit::info("scenes-info", None, hint)),
                        ),
                    )
                })
                .children(animate)
                .children(confirm)
                .children(ask)
                .children(cards)
                .children(provenance)
                .into_any_element(),
        )
    }

    /// The hint, estimate and button that animate every scene without a
    /// clip.
    fn render_animate_all(&self, view: &ScenesView, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let missing = view.clips.missing.len();
        let estimate = view
            .clips
            .missing_estimate
            .as_ref()
            .and_then(|estimate| estimate_note(bardo, estimate, Text::EstimateCost, cx));
        if missing == 0 {
            return div().into_any_element();
        }
        v_flex()
            .gap_1()
            .children(estimate)
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        Button::new("animate-missing-clips")
                            .outline()
                            .small()
                            .label(SharedString::from(bardo.text_with(
                                Text::AnimateMissingClips,
                                &[("n", &missing.to_string())],
                            )))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.run_scene(
                                    SceneAction::AnimateMissing,
                                    BudgetConsent::Ask,
                                    window,
                                    cx,
                                );
                            })),
                    )
                    .child(kit::info("clips-info", None, tr(bardo, Text::ClipsHint))),
            )
            .into_any_element()
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

    /// The image of a scene, or a box saying why there is none.
    fn render_thumbnail(&self, image: Option<&SceneImage>, cx: &App) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let frame = div()
            .w(px(THUMB_WIDTH))
            .h(px(THUMB_HEIGHT))
            .flex_none()
            .rounded_md()
            .overflow_hidden()
            .bg(theme.muted);
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
                .text_color(theme.muted_foreground)
                .child(tr(bardo, Text::SceneNoImage))
                .into_any_element(),
        }
    }

    fn render_scene(
        &self,
        index: usize,
        scene: &Scene,
        clip: Option<&SceneClipView>,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = self
            .editing_scene
            .filter(|(_, editing, _)| *editing == index)
            .map(|(_, _, field)| field);
        let editing = open == Some(ScenePromptField::Image);
        let clip = clip
            .filter(|_| scene.can_animate())
            .map(|clip| self.render_scene_clip(index, scene, clip, open, cx));
        let thumbnail = self.render_thumbnail(scene.image(), cx);
        let pending = scene
            .pending()
            .map(|pending| self.render_pending_image(index, pending, cx));
        let editor = editing.then(|| self.render_prompt_editor(index, None, cx));
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();

        let label = h_flex()
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
            });

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
        });

        let prompt: AnyElement = if let Some(editor) = editor {
            editor
        } else {
            kit::well(cx)
                .text_xs()
                .child(SharedString::from(scene.prompt().as_str().to_owned()))
                .into_any_element()
        };

        let record = scene.image().map(|image| {
            let generation = &image.generation;
            let usage = generation.usage;
            kit::details(
                ("scene-image-details", index),
                tr(bardo, Text::Details),
                vec![SharedString::from(bardo.text_with(
                    Text::SceneImageRecord,
                    &[
                        ("model", &generation.model),
                        (
                            "tokens",
                            &(usage.input_tokens + usage.output_tokens).to_string(),
                        ),
                        ("n", &generation.template.number.to_string()),
                    ],
                ))],
            )
        });

        // Drawing while the prompt is open would draw the saved prompt, not
        // the one being typed, so the actions wait for the editor to close.
        let actions = open.is_none().then(|| {
            h_flex()
                .gap_2()
                .child(
                    Button::new(("edit-scene-prompt", index))
                        .ghost()
                        .xsmall()
                        .label(tr(bardo, Text::EditScenePrompt))
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.edit_scene(index, ScenePromptField::Image, window, cx)
                        })),
                )
                .when(
                    scene.image().is_some() && !busy && scene.pending().is_none(),
                    |row| {
                        row.child(
                            Button::new(("regenerate-scene-image", index))
                                .outline()
                                .xsmall()
                                .label(tr(bardo, Text::RegenerateSceneImage))
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    this.run_scene(
                                        SceneAction::Redraw(index),
                                        BudgetConsent::Ask,
                                        window,
                                        cx,
                                    );
                                })),
                        )
                    },
                )
                .children(record)
        });

        kit::card(cx)
            .id(("scene", index))
            .p_3()
            .gap_2()
            .child(
                h_flex().gap_3().items_start().child(thumbnail).child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1p5()
                        .child(label)
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme.muted_foreground)
                                .child(SharedString::from(scene.text.clone())),
                        )
                        .child(prompt)
                        .children(failure)
                        .children(actions),
                ),
            )
            .children(pending)
            .children(clip)
            .into_any_element()
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
            .and_then(|scenes| scenes.clips.channel_model.clone());
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
            .when(scene.own_motion_prompt().is_none(), |row| {
                row.child(
                    Tag::secondary()
                        .small()
                        .child(tr(bardo, Text::SceneMotionFromImage)),
                )
            });

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
        let own = scene.clip_model().cloned();
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
                            Button::new(("animate-scene", index))
                                .outline()
                                .xsmall()
                                .label(tr(
                                    bardo,
                                    if scene.clip().is_some() || scene.pending_clip().is_some() {
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

        // A clip names its model as people call it, while still offered.
        let record = |clip: &SceneClip| {
            let model = models
                .iter()
                .find(|model| model.id.model() == clip.generation.model)
                .map_or(clip.generation.model.as_str(), |model| model.name.as_str());
            SharedString::from(bardo.text_with(
                Text::SceneClipRecord,
                &[("seconds", &clip.seconds.to_string()), ("model", model)],
            ))
        };
        let current = scene.clip().map(|clip| {
            let played = clip.clone();
            h_flex()
                .gap_2()
                .items_center()
                .child(Tag::secondary().small().child(record(clip)))
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
                        .items_center()
                        .child(
                            div()
                                .text_sm()
                                .font_medium()
                                .child(tr(bardo, Text::ScenePendingClipTitle)),
                        )
                        .child(Tag::secondary().small().child(record(clip)))
                        .child(kit::info(
                            ("pending-clip-info", index),
                            None,
                            tr(bardo, Text::ScenePendingClipHint),
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

        v_flex()
            .pt_2()
            .gap_1p5()
            .border_t_1()
            .border_color(theme.border)
            .child(motion_label)
            .child(motion)
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .text_xs()
                            .font_semibold()
                            .child(tr(bardo, Text::SceneClipModel)),
                    )
                    .child(model_menu)
                    .children(plan_note),
            )
            .children(model_gone)
            .children(state)
            .children(actions)
            .children(current)
            .children(pending)
            .into_any_element()
    }

    /// A scene's new image beside the choice to use it or keep the current.
    fn render_pending_image(
        &self,
        index: usize,
        image: &SceneImage,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let thumbnail = self.render_thumbnail(Some(image), cx);
        let bardo = self.bardo.read(cx);
        kit::card(cx)
            .flex_row()
            .p_2()
            .gap_3()
            .items_start()
            .border_color(look(cx).tokens.accent_edge)
            .child(thumbnail)
            .child(
                v_flex()
                    .gap_1p5()
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_sm()
                                    .font_medium()
                                    .child(tr(bardo, Text::ScenePendingTitle)),
                            )
                            .child(kit::info(
                                ("pending-image-info", index),
                                None,
                                tr(bardo, Text::ScenePendingHint),
                            )),
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
            .into_any_element()
    }
}
