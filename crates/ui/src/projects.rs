//! Video projects screen: pick a channel and one of its projects, then
//! generate, edit and review the project's script. Generation runs as a job
//! in `bardo_app`; this view polls the job revision and re-reads the script
//! when it moves. The editor keeps the user's typing: it is refilled only
//! when the stored text changes (first generation, accepting a new script).

use std::time::Duration;

use bardo_app::bardo_domain::{
    Channel, ChannelId, Generation, JobState, ScriptFieldError, VideoProject, VideoProjectId,
};
use bardo_app::{Bardo, ScriptView, Text};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::searchable_list::{SearchableListItem, SearchableVec};
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Entity, SharedString, Subscription, Task, Window, div, px,
};

use crate::shell::tr;

/// How often the screen checks the job queue for changes.
const POLL_EVERY: Duration = Duration::from_millis(100);

/// One option of the channel select.
#[derive(Clone)]
struct Choice {
    value: ChannelId,
    title: SharedString,
}

impl SearchableListItem for Choice {
    type Value = ChannelId;

    fn title(&self) -> SharedString {
        self.title.clone()
    }

    fn value(&self) -> &ChannelId {
        &self.value
    }
}

/// Which generation's prompt is open.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PromptShown {
    Source,
    Pending,
}

pub struct ProjectsScreen {
    bardo: Entity<Bardo>,
    channels: Vec<Channel>,
    channel_select: Entity<SelectState<SearchableVec<Choice>>>,
    channel: Option<ChannelId>,
    projects: Vec<VideoProject>,
    project: Option<VideoProjectId>,
    view: Option<ScriptView>,
    editor: Entity<TextareaState>,
    /// The stored text last placed in the editor.
    loaded: Option<String>,
    field_error: Option<ScriptFieldError>,
    error: Option<Text>,
    notice: Option<Text>,
    prompt_shown: Option<PromptShown>,
    revision: u64,
    _poll: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl ProjectsScreen {
    pub fn new(bardo: Entity<Bardo>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let channel_select =
            cx.new(|cx| SelectState::new(SearchableVec::new(Vec::new()), None, window, cx));
        let editor = cx.new(|cx| TextareaState::new(window, cx).auto_grow(12, 24));
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL_EVERY).await;
                if this
                    .update_in(cx, |this, window, cx| this.poll(window, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        let subscriptions = vec![cx.subscribe_in(
            &channel_select,
            window,
            |this, _, event: &SelectEvent<SearchableVec<Choice>>, window, cx| {
                let SelectEvent::Confirm(Some(id)) = event else {
                    return;
                };
                if this.channel != Some(*id) {
                    this.select_channel(*id, window, cx);
                }
            },
        )];
        let revision = bardo.read(cx).jobs_revision();
        let mut screen = Self {
            bardo,
            channels: Vec::new(),
            channel_select,
            channel: None,
            projects: Vec::new(),
            project: None,
            view: None,
            editor,
            loaded: None,
            field_error: None,
            error: None,
            notice: None,
            prompt_shown: None,
            revision,
            _poll: poll,
            _subscriptions: subscriptions,
        };
        screen.reload(window, cx);
        screen
    }

    /// Re-reads channels and projects (themes approved elsewhere start new
    /// ones), keeping the selection when it still exists.
    pub fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.error = None;
        self.notice = None;
        self.channels = self.bardo.read(cx).channels().unwrap_or_default();
        let choices = SearchableVec::new(
            self.channels
                .iter()
                .map(|channel| Choice {
                    value: channel.id,
                    title: SharedString::from(channel.details.name().to_owned()),
                })
                .collect::<Vec<_>>(),
        );
        let keep = self
            .channel
            .filter(|id| self.channels.iter().any(|c| c.id == *id));
        let target = keep.or_else(|| self.channels.first().map(|c| c.id));
        self.channel_select.update(cx, |select, cx| {
            select.set_items(choices, window, cx);
            if let Some(id) = target {
                select.set_selected_value(&id, window, cx);
            }
        });
        match target {
            Some(_) if keep.is_some() => {
                self.load_projects(cx);
                self.load(window, cx);
            }
            Some(id) => self.select_channel(id, window, cx),
            None => {
                self.channel = None;
                self.projects.clear();
                self.project = None;
                self.view = None;
            }
        }
        cx.notify();
    }

    fn select_channel(&mut self, id: ChannelId, window: &mut Window, cx: &mut Context<Self>) {
        self.channel = Some(id);
        self.project = None;
        self.load_projects(cx);
        let first = self.projects.first().map(|project| project.id);
        self.select_project(first, window, cx);
    }

    fn load_projects(&mut self, cx: &mut Context<Self>) {
        let Some(channel) = self.channel else {
            return;
        };
        match self.bardo.read(cx).video_projects(channel) {
            Ok(projects) => self.projects = projects,
            Err(_) => {
                self.projects.clear();
                self.error = Some(Text::ProjectsNotLoaded);
            }
        }
        if self
            .project
            .is_some_and(|id| !self.projects.iter().any(|p| p.id == id))
        {
            self.project = self.projects.first().map(|project| project.id);
        }
    }

    fn select_project(
        &mut self,
        id: Option<VideoProjectId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.project = id;
        self.loaded = None;
        self.field_error = None;
        self.error = None;
        self.notice = None;
        self.prompt_shown = None;
        self.load(window, cx);
        cx.notify();
    }

    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.project else {
            self.view = None;
            return;
        };
        match self.bardo.read(cx).script(id) {
            Ok(view) => {
                let stored = view
                    .script
                    .as_ref()
                    .map(|script| script.text().as_str().to_owned());
                if stored != self.loaded {
                    let text = stored.clone().unwrap_or_default();
                    self.editor
                        .update(cx, |input, cx| input.set_value(text, window, cx));
                    self.loaded = stored;
                }
                self.view = Some(view);
            }
            Err(error) => {
                self.view = None;
                self.error = Some(error.message());
            }
        }
    }

    fn poll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let revision = self.bardo.read(cx).jobs_revision();
        if revision != self.revision {
            self.revision = revision;
            self.load(window, cx);
            cx.notify();
        }
    }

    fn running(&self) -> bool {
        self.view
            .as_ref()
            .and_then(|view| view.job.as_ref())
            .is_some_and(|job| job.state().is_active())
    }

    fn generate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.project else {
            return;
        };
        self.error = self
            .bardo
            .read(cx)
            .generate_script(id)
            .err()
            .map(|error| error.message());
        self.notice = None;
        self.load(window, cx);
        cx.notify();
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.project else {
            return;
        };
        let text = self.editor.read(cx).value();
        match self.bardo.read(cx).edit_script(id, &text) {
            Ok(script) => {
                self.field_error = None;
                self.error = None;
                self.notice = Some(Text::ScriptSaved);
                // The editor already shows the saved text; refilling it
                // would only move the caret.
                self.loaded = Some(script.text().as_str().to_owned());
            }
            Err(error) => {
                self.field_error = error.field_error();
                self.error = self.field_error.is_none().then(|| error.message());
                self.notice = None;
            }
        }
        self.load(window, cx);
        cx.notify();
    }

    fn revert(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.loaded = None;
        self.field_error = None;
        self.notice = None;
        self.load(window, cx);
        cx.notify();
    }

    fn review(&mut self, accept: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.project else {
            return;
        };
        let bardo = self.bardo.read(cx);
        let result = if accept {
            bardo.accept_script(id)
        } else {
            bardo.reject_script(id)
        };
        self.error = result.err().map(|error| error.message());
        self.notice = None;
        self.prompt_shown = None;
        self.load(window, cx);
        cx.notify();
    }

    fn render_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let rows: Vec<AnyElement> = self
            .projects
            .iter()
            .enumerate()
            .map(|(ix, project)| {
                let id = project.id;
                let selected = self.project == Some(id);
                v_flex()
                    .id(("project", ix))
                    .px_3()
                    .py_2()
                    .gap_0p5()
                    .rounded_md()
                    .cursor_pointer()
                    .when(selected, |row| row.bg(theme.list_active))
                    .when(!selected, |row| row.hover(|row| row.bg(theme.list_hover)))
                    .child(SharedString::from(project.title.clone()))
                    .child(div().text_xs().text_color(theme.muted_foreground).child(
                        SharedString::from(format!(
                            "{} · {}",
                            project.niche.label(),
                            bardo.time_ago(project.created_at)
                        )),
                    ))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        if this.project != Some(id) {
                            this.select_project(Some(id), window, cx);
                        }
                    }))
                    .into_any_element()
            })
            .collect();
        let empty = rows.is_empty();

        v_flex()
            .id("projects-list")
            .w(px(300.))
            .h_full()
            .flex_none()
            .overflow_y_scroll()
            .p_4()
            .gap_3()
            .border_r_1()
            .border_color(theme.border)
            .child(
                div()
                    .text_xl()
                    .font_semibold()
                    .child(tr(bardo, Text::ProjectsTitle)),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .font_medium()
                            .child(tr(bardo, Text::ThemesChannel)),
                    )
                    .child(Select::new(&self.channel_select)),
            )
            .when(empty, |list| {
                list.child(muted(cx, tr(bardo, Text::ProjectsEmpty)))
            })
            .children(rows)
    }

    fn render_script(&self, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let Some(view) = &self.view else {
            return v_flex()
                .flex_1()
                .p_4()
                .children(self.error.map(|error| {
                    div()
                        .text_sm()
                        .text_color(theme.danger)
                        .child(tr(bardo, error))
                }))
                .into_any_element();
        };
        let running = self.running();
        let project = &view.project;
        let script = view.script.as_ref();

        let header =
            v_flex()
                .gap_0p5()
                .child(
                    div()
                        .text_xl()
                        .font_semibold()
                        .child(SharedString::from(project.title.clone())),
                )
                .child(div().text_xs().text_color(theme.muted_foreground).child(
                    SharedString::from(format!(
                        "{} · {}",
                        project.niche.label(),
                        bardo.time_ago(project.created_at)
                    )),
                ));

        let title_row = h_flex()
            .gap_2()
            .items_center()
            .child(
                div()
                    .text_lg()
                    .font_semibold()
                    .child(tr(bardo, Text::ScriptTitle)),
            )
            .children(script.map(|script| {
                Tag::secondary()
                    .small()
                    .child(SharedString::from(bardo.text_with(
                        Text::ScriptWords,
                        &[("n", &script.text().word_count().to_string())],
                    )))
            }))
            .when(script.is_some_and(|script| script.is_edited()), |row| {
                row.child(Tag::warning().small().child(tr(bardo, Text::ScriptEdited)))
            });

        let body: AnyElement = match script {
            None => v_flex()
                .gap_2()
                .child(muted(cx, tr(bardo, Text::ScriptEmpty)))
                .child(div().text_xs().text_color(theme.muted_foreground).child(
                    SharedString::from(bardo.text_with(
                        Text::GenerateScriptHint,
                        &[("n", &view.template.number.to_string())],
                    )),
                ))
                .child(
                    h_flex().child(
                        Button::new("generate-script")
                            .primary()
                            .label(tr(bardo, Text::GenerateScript))
                            .disabled(running)
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.generate(window, cx)
                            })),
                    ),
                )
                .into_any_element(),
            Some(script) => v_flex()
                .gap_2()
                .when(script.pending().is_some(), |body| {
                    body.child(div().font_medium().child(tr(bardo, Text::ScriptCurrent)))
                })
                .child(Textarea::new(&self.editor))
                .children(self.field_error.map(|error| {
                    div()
                        .text_xs()
                        .text_color(theme.danger)
                        .child(tr(bardo, Text::ScriptFieldError(error)))
                }))
                .child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .items_center()
                        .child(
                            Button::new("save-script")
                                .primary()
                                .small()
                                .label(tr(bardo, Text::SaveScript))
                                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                    this.save(window, cx)
                                })),
                        )
                        .child(
                            Button::new("revert-script")
                                .ghost()
                                .small()
                                .label(tr(bardo, Text::RevertScript))
                                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                    this.revert(window, cx)
                                })),
                        )
                        .child(div().flex_1())
                        .child(
                            Button::new("regenerate-script")
                                .outline()
                                .small()
                                .label(tr(bardo, Text::RegenerateScript))
                                .disabled(running)
                                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                    this.generate(window, cx)
                                })),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(tr(bardo, Text::RegenerateScriptHint)),
                )
                .into_any_element(),
        };

        v_flex()
            .id("projects-script")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .p_4()
            .gap_3()
            .child(header)
            .child(title_row)
            .children(self.notice.map(|notice| {
                div()
                    .text_sm()
                    .text_color(theme.success)
                    .child(tr(bardo, notice))
            }))
            .children(self.error.map(|error| {
                div()
                    .text_sm()
                    .text_color(theme.danger)
                    .child(tr(bardo, error))
            }))
            .children(self.render_job(cx))
            .children(script.and_then(|s| s.pending()).map(|pending| {
                self.render_pending(pending.text().as_str(), pending.generation(), cx)
            }))
            .child(body)
            .children(script.map(|script| {
                self.render_provenance(script.source().generation(), PromptShown::Source, cx)
            }))
            .into_any_element()
    }

    /// A running generation, or why the last one stopped.
    fn render_job(&self, cx: &App) -> Option<AnyElement> {
        let job = self.view.as_ref()?.job.as_ref()?;
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        match job.state() {
            JobState::Queued | JobState::Running => Some(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(Spinner::new().small())
                    .child(div().text_sm().child(tr(bardo, Text::ScriptRunning)))
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
                                .child(tr(bardo, Text::ScriptStopped)),
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

    fn render_pending(
        &self,
        text: &str,
        generation: &Generation,
        cx: &Context<Self>,
    ) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        v_flex()
            .p_3()
            .gap_2()
            .rounded_md()
            .border_1()
            .border_color(theme.primary)
            .child(
                div()
                    .font_medium()
                    .child(tr(bardo, Text::ScriptPendingTitle)),
            )
            .child(
                div()
                    .id("pending-script")
                    .max_h(px(260.))
                    .overflow_y_scroll()
                    .p_2()
                    .rounded_md()
                    .bg(theme.muted)
                    .text_sm()
                    .child(SharedString::from(text.to_owned())),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Button::new("accept-script")
                            .primary()
                            .small()
                            .label(tr(bardo, Text::AcceptScript))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.review(true, window, cx)
                            })),
                    )
                    .child(
                        Button::new("reject-script")
                            .ghost()
                            .small()
                            .label(tr(bardo, Text::RejectScript))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.review(false, window, cx)
                            })),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(tr(bardo, Text::ScriptPendingHint)),
                    ),
            )
            .child(self.render_provenance(generation, PromptShown::Pending, cx))
            .into_any_element()
    }

    /// Who generated the text, from what, and what it used.
    fn render_provenance(
        &self,
        generation: &Generation,
        which: PromptShown,
        cx: &Context<Self>,
    ) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let shown = self.prompt_shown == Some(which);
        let fact = |label: Text, value: String| {
            v_flex()
                .gap_0p5()
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(tr(bardo, label)),
                )
                .child(div().text_sm().child(SharedString::from(value)))
        };
        let prompt_block = |label: Text, text: &str| {
            v_flex()
                .gap_1()
                .child(div().text_xs().font_medium().child(tr(bardo, label)))
                .child(
                    div()
                        .p_2()
                        .rounded_md()
                        .bg(theme.muted)
                        .text_xs()
                        .child(SharedString::from(text.to_owned())),
                )
        };
        let usage = generation.usage;

        v_flex()
            .pt_3()
            .gap_2()
            .border_t_1()
            .border_color(theme.border)
            .child(
                div()
                    .text_sm()
                    .font_medium()
                    .child(tr(bardo, Text::ProvenanceTitle)),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_x_6()
                    .gap_y_2()
                    .child(fact(
                        Text::ProvenanceProvider,
                        bardo
                            .text(Text::ProviderName(generation.provider))
                            .into_owned(),
                    ))
                    .child(fact(Text::ProvenanceModel, generation.model.clone()))
                    .child(fact(
                        Text::ProvenanceTemplate,
                        format!(
                            "{} {}",
                            bardo.text(Text::TemplateKindName(
                                bardo_app::bardo_domain::TemplateKind::Script
                            )),
                            bardo.text_with(
                                Text::ProvenanceTemplateVersion,
                                &[("n", &generation.template.number.to_string())],
                            )
                        ),
                    ))
                    .child(fact(
                        Text::ProvenanceTokens,
                        bardo.text_with(
                            Text::ProvenanceTokensValue,
                            &[
                                ("input", &usage.input_tokens.to_string()),
                                ("output", &usage.output_tokens.to_string()),
                            ],
                        ),
                    ))
                    .child(fact(
                        Text::ProvenanceGenerated,
                        bardo.time_ago(generation.generated_at),
                    )),
            )
            .child(
                h_flex().child(
                    Button::new(match which {
                        PromptShown::Source => "toggle-source-prompt",
                        PromptShown::Pending => "toggle-pending-prompt",
                    })
                    .ghost()
                    .xsmall()
                    .label(tr(
                        bardo,
                        if shown {
                            Text::HidePrompt
                        } else {
                            Text::ShowPrompt
                        },
                    ))
                    .on_click(cx.listener(
                        move |this, _: &ClickEvent, _, cx| {
                            this.prompt_shown = if this.prompt_shown == Some(which) {
                                None
                            } else {
                                Some(which)
                            };
                            cx.notify();
                        },
                    )),
                ),
            )
            .when(shown, |panel| {
                panel
                    .child(prompt_block(
                        Text::PromptInstructions,
                        &generation.instructions,
                    ))
                    .child(prompt_block(Text::PromptTask, &generation.prompt))
            })
            .into_any_element()
    }
}

fn muted(cx: &App, text: SharedString) -> AnyElement {
    div()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(text)
        .into_any_element()
}

impl Render for ProjectsScreen {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.channels.is_empty() {
            let bardo = self.bardo.read(cx);
            return h_flex()
                .size_full()
                .p_6()
                .items_start()
                .child(muted(cx, tr(bardo, Text::ProjectsNoChannels)))
                .into_any_element();
        }
        h_flex()
            .size_full()
            .items_start()
            .child(self.render_list(cx))
            .child(self.render_script(cx))
            .into_any_element()
    }
}
