//! The scenes panel of the projects screen: plan the narration's scenes,
//! read and edit each scene's image prompt, draw the images, draw one again
//! and choose between the current image and the new one.

use bardo_app::bardo_domain::{Job, JobState, Scene, SceneImage, TemplateKind, VideoProjectId};
use bardo_app::{SceneError, Text};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Textarea;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, ClickEvent, ObjectFit, SharedString, Window, div, img, px};

use super::{ProjectsScreen, PromptShown, clock, muted};
use crate::shell::tr;

/// Thumbnails are 16:9, like the images.
const THUMB_WIDTH: f32 = 192.;
const THUMB_HEIGHT: f32 = 108.;

impl ProjectsScreen {
    pub(super) fn load_scenes(&mut self, id: VideoProjectId, cx: &mut Context<Self>) {
        match self.bardo.read(cx).scenes(id) {
            Ok(view) => {
                // The scene being edited went with a new plan.
                let plan = view.plan.as_ref().map(|plan| plan.id);
                if self.editing_scene.is_some_and(|(id, _)| Some(id) != plan) {
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

    fn plan_scenes(&mut self, discard_images: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_replan = false;
        let result = self.scene_action(window, cx, |bardo, id| {
            bardo.plan_scenes(id, discard_images)
        });
        if result.is_none()
            && self
                .scenes_error
                .is_some_and(|error| error == Text::ScenesWouldDiscardImages)
        {
            // Ask in the panel instead of showing the error.
            self.scenes_error = None;
            self.confirm_replan = true;
        }
    }

    fn edit_scene(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(plan) = self.scenes.as_ref().and_then(|view| view.plan.as_ref()) else {
            return;
        };
        let Ok(scene) = plan.scene(index) else {
            return;
        };
        let text = scene.prompt().as_str().to_owned();
        self.editing_scene = Some((plan.id, index));
        self.scene_field_error = None;
        self.scene_editor
            .update(cx, |input, cx| input.set_value(text, window, cx));
        cx.notify();
    }

    fn save_scene_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, index)) = self.editing_scene else {
            return;
        };
        let text = self.scene_editor.read(cx).value();
        let saved = self.scene_action(window, cx, |bardo, id| {
            bardo.edit_scene_prompt(id, index, &text)
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
                    .map(|(index, scene)| self.render_scene(index, scene, busy, cx))
                    .collect()
            })
            .unwrap_or_default();
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
            .child(
                div()
                    .text_lg()
                    .font_semibold()
                    .child(tr(bardo, Text::ScenesTitle)),
            )
            .children(plan.map(|plan| {
                Tag::secondary()
                    .small()
                    .child(SharedString::from(bardo.text_with(
                        Text::ScenesCount,
                        &[("n", &plan.scenes().len().to_string())],
                    )))
            }))
            .when(view.stale, |row| {
                row.child(
                    Tag::warning()
                        .small()
                        .child(tr(bardo, Text::ScenesStaleTag)),
                )
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
            .disabled(busy || view.narration.is_none())
            .on_click(
                cx.listener(|this, _: &ClickEvent, window, cx| this.plan_scenes(false, window, cx)),
            );
        let plan_button = if plan.is_none() || view.stale {
            plan_button.primary()
        } else {
            plan_button.outline()
        };
        let missing = view.missing_images();
        let draw_button = (missing > 0).then(|| {
            Button::new("generate-scene-images")
                .primary()
                .small()
                .label(SharedString::from(bardo.text_with(
                    Text::GenerateSceneImages,
                    &[("n", &missing.to_string())],
                )))
                .disabled(busy)
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.scene_action(window, cx, |bardo, id| bardo.generate_scene_images(id));
                }))
        });
        let files = plan.map_or(0, |plan| plan.files().count());
        let confirm = self.confirm_replan.then(|| {
            v_flex()
                .p_3()
                .gap_2()
                .rounded_md()
                .border_1()
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
                                    this.plan_scenes(true, window, cx)
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
        let hint = match (view.narration, plan) {
            (None, _) => tr(bardo, Text::ScenesNoNarration),
            (Some(_), None) => SharedString::from(bardo.text_with(
                Text::PlanScenesHint,
                &[("n", &view.template.number.to_string())],
            )),
            (Some(_), Some(_)) => tr(bardo, Text::SceneImagesHint),
        };

        Some(
            v_flex()
                .pt_3()
                .gap_2()
                .border_t_1()
                .border_color(theme.border)
                .child(title_row)
                .when(view.stale, |panel| {
                    panel.child(
                        div()
                            .text_sm()
                            .text_color(theme.warning)
                            .child(tr(bardo, Text::ScenesStale)),
                    )
                })
                .children(self.scenes_error.map(|error| {
                    div()
                        .text_sm()
                        .text_color(theme.danger)
                        .child(tr(bardo, error))
                }))
                .children(job)
                .when(plan.is_none(), |panel| {
                    panel.child(muted(cx, tr(bardo, Text::ScenesEmpty)))
                })
                .child(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(hint),
                        )
                        .child(h_flex().gap_2().children(draw_button).child(plan_button)),
                )
                .children(confirm)
                .children(cards)
                .children(provenance)
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
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme.danger)
                                .child(tr(bardo, Text::ScenesStopped)),
                        )
                        .child(
                            div()
                                .text_xs()
                                .child(tr(bardo, Text::JobFailureKindName(failure.kind))),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(SharedString::from(failure.detail.clone())),
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
        busy: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let editing = self
            .editing_scene
            .is_some_and(|(_, editing)| editing == index);
        let thumbnail = self.render_thumbnail(scene.image(), cx);
        let pending = scene
            .pending()
            .map(|pending| self.render_pending_image(index, pending, cx));
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
                row.child(Tag::warning().small().child(tr(bardo, Text::SceneEdited)))
            });

        let failure = scene.failure().map(|kind| {
            div()
                .text_xs()
                .text_color(theme.danger)
                .child(SharedString::from(format!(
                    "{} {}",
                    bardo.text(Text::SceneFailed),
                    bardo.text(Text::JobFailureKindName(kind))
                )))
        });

        let prompt: AnyElement = if editing {
            v_flex()
                .gap_1()
                .child(Textarea::new(&self.scene_editor))
                .children(self.scene_field_error.map(|error| {
                    div()
                        .text_xs()
                        .text_color(theme.danger)
                        .child(tr(bardo, Text::SceneFieldError(error)))
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
        } else {
            div()
                .p_2()
                .rounded_md()
                .bg(theme.muted)
                .text_xs()
                .child(SharedString::from(scene.prompt().as_str().to_owned()))
                .into_any_element()
        };

        // Drawing while the prompt is open would draw the saved prompt, not
        // the one being typed, so the actions wait for the editor to close.
        let actions = (!editing).then(|| {
            h_flex()
                .gap_2()
                .child(
                    Button::new(("edit-scene-prompt", index))
                        .ghost()
                        .xsmall()
                        .label(tr(bardo, Text::EditScenePrompt))
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.edit_scene(index, window, cx)
                        })),
                )
                .when(scene.image().is_some(), |row| {
                    row.child(
                        Button::new(("regenerate-scene-image", index))
                            .outline()
                            .xsmall()
                            .label(tr(bardo, Text::RegenerateSceneImage))
                            .disabled(busy || scene.pending().is_some())
                            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                this.scene_action(window, cx, |bardo, id| {
                                    bardo.regenerate_scene_image(id, index)
                                });
                            })),
                    )
                })
        });

        let record = scene.image().map(|image| {
            let generation = &image.generation;
            let usage = generation.usage;
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(SharedString::from(bardo.text_with(
                    Text::SceneImageRecord,
                    &[
                        ("model", &generation.model),
                        (
                            "tokens",
                            &(usage.input_tokens + usage.output_tokens).to_string(),
                        ),
                        ("n", &generation.template.number.to_string()),
                    ],
                )))
        });

        v_flex()
            .id(("scene", index))
            .p_3()
            .gap_2()
            .rounded_md()
            .border_1()
            .border_color(theme.border)
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
                        .children(actions)
                        .children(record),
                ),
            )
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
        let theme = cx.theme();
        h_flex()
            .p_2()
            .gap_3()
            .items_start()
            .rounded_md()
            .border_1()
            .border_color(theme.primary)
            .child(thumbnail)
            .child(
                v_flex()
                    .gap_1p5()
                    .child(
                        div()
                            .text_sm()
                            .font_medium()
                            .child(tr(bardo, Text::ScenePendingTitle)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(tr(bardo, Text::ScenePendingHint)),
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
