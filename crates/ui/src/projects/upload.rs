//! Upload with review at the Publish stage (#77). For a network Bardo
//! uploads to, the inspector offers "Review upload": a card with the file,
//! the connected channel, the title, description and tags as the network
//! gets them, the visibility, made for kids and the synthetic-content
//! disclosure. Nothing is sent until the user confirms the card; a change
//! after they opened it sends them back to a new review. The upload then
//! runs as a job the stage follows: stop, resume and retry, with the state
//! the post section shows.

use bardo_app::bardo_domain::{JobId, PublicationId, Visibility};
use bardo_app::{Text, UploadChoices, UploadReview, UploadReviewError, UploadState};
use gpui_kit::component::button::{Button, ButtonGroup, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::{
    Disableable as _, Selectable as _, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, ClickEvent, SharedString, Window, div};

use super::{ProjectsScreen, clock, muted};
use crate::appearance::look;
use crate::kit::{self, Tone};
use crate::shell::tr;

fn label(text: SharedString) -> gpui_kit::Div {
    div().text_xs().font_semibold().child(text)
}

impl ProjectsScreen {
    /// Reads the shown network's upload review, when Bardo uploads to it.
    /// An open review of another network closes.
    pub(super) fn load_upload(&mut self, cx: &mut Context<Self>) {
        let bardo = self.bardo.read(cx);
        let shown = self
            .export_view
            .as_ref()
            .and_then(|view| self.shown_network(view))
            .map(|target| target.network);
        self.upload_now = match (self.project, shown) {
            (Some(project), Some(network)) if bardo.uploads_to(network) => {
                bardo.upload_review(project, network).ok()
            }
            _ => None,
        };
        if self
            .upload_draft
            .as_ref()
            .is_some_and(|(review, _)| Some(review.network) != shown)
        {
            self.upload_draft = None;
        }
    }

    fn open_upload_review(&mut self, cx: &mut Context<Self>) {
        if let Some(review) = self.upload_now.clone() {
            let choices = review.choices();
            self.upload_draft = Some((review, choices));
            self.upload_error = None;
        }
        cx.notify();
    }

    fn change_upload(&mut self, change: impl FnOnce(&mut UploadChoices), cx: &mut Context<Self>) {
        if let Some((_, choices)) = self.upload_draft.as_mut() {
            change(choices);
        }
        cx.notify();
    }

    /// Starts the upload the user reviewed. A review that no longer holds
    /// closes, so the next one shows what changed.
    fn confirm_upload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((review, choices)) = self.upload_draft.clone() else {
            return;
        };
        match self.bardo.read(cx).start_upload(&review, choices) {
            Ok(_) => {
                self.upload_draft = None;
                self.upload_error = None;
                self.export_notice = Some(Text::UploadQueued);
            }
            Err(error) => {
                if matches!(
                    error,
                    UploadReviewError::Changed | UploadReviewError::Blocked(_)
                ) {
                    self.upload_draft = None;
                }
                self.upload_error = Some(error.message());
            }
        }
        self.load(window, cx);
        cx.notify();
    }

    fn upload_job_action(
        &mut self,
        job: JobId,
        resume: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let bardo = self.bardo.read(cx);
        self.upload_error = if resume {
            bardo.resume_upload(job).err().map(|error| error.message())
        } else {
            bardo.cancel_job(job).err().map(|_| Text::UploadNotStarted)
        };
        self.load(window, cx);
        cx.notify();
    }

    fn check_upload_again(
        &mut self,
        publication: PublicationId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let bardo = self.bardo.read(cx);
        self.upload_error = bardo
            .check_upload(publication)
            .err()
            .map(|error| error.message());
        self.load(window, cx);
        cx.notify();
    }

    /// The shown network's upload: the open review, or what the upload
    /// does now and the way to review one. `None` for a network Bardo
    /// does not upload to.
    pub(super) fn upload_section(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let now = self.upload_now.as_ref()?;
        let bardo = self.bardo.read(cx);
        let heading = h_flex()
            .gap_1()
            .items_center()
            .child(label(tr(bardo, Text::UploadTitle)))
            .child(kit::info("upload-info", None, tr(bardo, Text::UploadHint)));
        let mut section = v_flex().gap_2().child(heading);
        if let Some((review, choices)) = &self.upload_draft {
            return Some(
                section
                    .child(self.upload_card(review, *choices, cx))
                    .into_any_element(),
            );
        }

        // The current upload's job: stop it, or resume or retry it.
        let upload = now.replaces.as_ref().and_then(|publication| {
            Some((publication.upload()?.job, bardo.upload_state(publication)?))
        });
        let shown = now.replaces.as_ref().map(|publication| publication.id);
        let active = upload.as_ref().is_some_and(|(_, state)| state.is_active());
        let mut actions = h_flex().gap_2().items_center().flex_wrap();
        match upload {
            Some((job, _)) if active => {
                actions = actions.child(
                    Button::new("upload-stop")
                        .small()
                        .outline()
                        .label(tr(bardo, Text::UploadStop))
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.upload_job_action(job, false, window, cx);
                        })),
                );
            }
            Some((job, UploadState::Stopped)) => {
                actions = actions.child(
                    Button::new("upload-resume")
                        .small()
                        .primary()
                        .label(tr(bardo, Text::UploadResume))
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.upload_job_action(job, true, window, cx);
                        })),
                );
            }
            Some((
                job,
                UploadState::Failed {
                    retryable: true, ..
                },
            )) => {
                actions = actions.child(
                    Button::new("upload-retry")
                        .small()
                        .outline()
                        .label(tr(bardo, Text::UploadRetry))
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.upload_job_action(job, true, window, cx);
                        })),
                );
            }
            Some((_, UploadState::StillProcessing)) => {
                actions = actions.child(
                    Button::new("upload-check")
                        .small()
                        .outline()
                        .label(tr(bardo, Text::UploadCheckAgain))
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            if let Some(publication) = shown {
                                this.check_upload_again(publication, window, cx);
                            }
                        })),
                );
            }
            _ => {}
        }
        let block = now.block();
        if !active {
            actions = actions.child(
                Button::new("upload-review")
                    .small()
                    .when(block.is_none(), |button| button.primary())
                    .when(block.is_some(), |button| button.outline())
                    .label(tr(bardo, Text::UploadOpenReview))
                    .disabled(block.is_some())
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.open_upload_review(cx);
                    })),
            );
        }
        section = section.child(actions);
        if let Some(block) = block.filter(|_| !active) {
            section = section.child(muted(cx, tr(bardo, Text::UploadBlocked(block))));
        }
        section = section.children(
            self.upload_error
                .map(|error| kit::notice(Tone::Danger, tr(bardo, error), cx)),
        );
        Some(section.into_any_element())
    }

    /// The review: what goes, where, and the choices the user makes.
    fn upload_card(
        &self,
        review: &UploadReview,
        choices: UploadChoices,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let network = bardo.text(Text::NetworkName(review.network)).into_owned();
        let with_network = |text: Text| bardo.text_with(text, &[("network", &network)]);
        let row = |name: Text, value: AnyElement| {
            v_flex()
                .gap_1()
                .child(label(tr(bardo, name)))
                .child(value)
                .into_any_element()
        };
        let text = |value: String| {
            div()
                .text_sm()
                .child(SharedString::from(value))
                .into_any_element()
        };

        let mut card = kit::card(cx)
            .p_3()
            .gap_3()
            .border_color(look(cx).tokens.accent_edge)
            .child(
                div()
                    .font_semibold()
                    .child(SharedString::from(with_network(Text::UploadReviewTitle))),
            );
        if let Some(render) = &review.render {
            let (width, height) = render.preset.dimensions();
            card = card.child(row(
                Text::UploadFieldFile,
                text(format!(
                    "{} · {width}×{height} · {} · {}",
                    render.file,
                    clock(render.duration),
                    bardo.render_file_line(render)
                )),
            ));
        }
        card = card.child(row(
            Text::UploadFieldChannel,
            text(format!(
                "{} · @{}",
                review.channel().unwrap_or_default(),
                review.handle
            )),
        ));
        if let Some(post) = &review.post {
            if let Some(title) = &post.title {
                card = card.child(row(Text::MetadataFieldTitle, text(title.clone())));
            }
            if let Some(description) = &post.text {
                card = card.child(row(
                    Text::MetadataFieldDescription,
                    kit::well(cx)
                        .text_xs()
                        .child(SharedString::from(description.clone()))
                        .into_any_element(),
                ));
            }
            card = card.child(row(Text::MetadataFieldTags, text(post.tags.join(", "))));
        }

        let visibility = ButtonGroup::new("upload-visibility")
            .outline()
            .small()
            .children(Visibility::ALL.map(|visibility| {
                Button::new(visibility.code())
                    .label(tr(bardo, Text::VisibilityName(visibility)))
                    .selected(visibility == choices.visibility)
            }))
            .on_click(cx.listener(|this, clicked: &Vec<usize>, _, cx| {
                if let Some(visibility) = clicked.first().and_then(|&i| Visibility::ALL.get(i)) {
                    let visibility = *visibility;
                    this.change_upload(|choices| choices.visibility = visibility, cx);
                }
            }));
        card = card.child(row(
            Text::UploadFieldVisibility,
            h_flex().child(visibility).into_any_element(),
        ));

        let screen = cx.entity().downgrade();
        let kids = Checkbox::new("upload-kids")
            .label(tr(bardo, Text::UploadMadeForKids))
            .checked(choices.made_for_kids)
            .on_click({
                let screen = screen.clone();
                move |checked: &bool, _, cx| {
                    let checked = *checked;
                    let _ = screen.update(cx, |this, cx| {
                        this.change_upload(|choices| choices.made_for_kids = checked, cx)
                    });
                }
            });
        card = card.child(h_flex().gap_1().items_center().child(kids).child(kit::info(
            "upload-kids-info",
            None,
            tr(bardo, Text::UploadMadeForKidsHint),
        )));
        let synthetic = Checkbox::new("upload-synthetic")
            .label(tr(bardo, Text::UploadSynthetic))
            .checked(choices.synthetic)
            .on_click({
                let screen = screen.clone();
                move |checked: &bool, _, cx| {
                    let checked = *checked;
                    let _ = screen.update(cx, |this, cx| {
                        this.change_upload(|choices| choices.synthetic = checked, cx)
                    });
                }
            });
        card = card.child(
            v_flex()
                .gap_1()
                .child(
                    h_flex()
                        .gap_1()
                        .items_center()
                        .child(synthetic)
                        .child(kit::info(
                            "upload-synthetic-info",
                            None,
                            tr(bardo, Text::UploadSyntheticHint),
                        )),
                )
                .when(review.synthetic, |column| {
                    column.child(muted(cx, tr(bardo, Text::UploadSyntheticOn)))
                }),
        );

        if let Some(replaced) = &review.replaces {
            let text = if replaced.upload().is_some() {
                Text::UploadReplaceUpload
            } else {
                Text::UploadReplacePost
            };
            let replace = Checkbox::new("upload-replace")
                .label(SharedString::from(with_network(text)))
                .checked(choices.replace)
                .on_click(move |checked: &bool, _, cx| {
                    let checked = *checked;
                    let _ = screen.update(cx, |this, cx| {
                        this.change_upload(|choices| choices.replace = checked, cx)
                    });
                });
            card = card.child(replace);
        }

        card = card.child(kit::notice(
            Tone::Warning,
            with_network(Text::UploadIrreversible),
            cx,
        ));
        let ready = review.replaces.is_none() || choices.replace;
        card = card.child(
            h_flex()
                .gap_2()
                .justify_end()
                .child(
                    Button::new("upload-back")
                        .small()
                        .ghost()
                        .label(tr(bardo, Text::UploadBack))
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.upload_draft = None;
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("upload-confirm")
                        .small()
                        .primary()
                        .label(tr(bardo, Text::UploadStart))
                        .disabled(!ready)
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.confirm_upload(window, cx);
                        })),
                ),
        );
        card.into_any_element()
    }
}
