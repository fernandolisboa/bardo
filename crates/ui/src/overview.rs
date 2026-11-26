//! Overview: the open project at a glance. Its videos in progress, each
//! with the stage it is at and the step that moves it on (a button that
//! goes there), the ideas waiting for the user, and beside them the
//! project's channels, this month's spending and its narrators. What it
//! shows comes from `Bardo::overview`; this screen only lays it out.

use bardo_app::bardo_domain::VideoProjectId;
use bardo_app::{Bardo, Control, Destination, NextStep, OverviewView, Stage, Text, TourAnchor};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Entity, EventEmitter, SharedString, Window, div};

use crate::appearance::look;
use crate::kit::{self, Tone};
use crate::parts::{Header, ScreenParts};
use crate::shell::tr;
use crate::{guide, layout};

/// Open a video at a stage (Edit opens the editor).
pub struct OpenVideo {
    pub video: VideoProjectId,
    pub stage: Option<Stage>,
}

/// Open Ideas › Themes.
pub struct OpenIdeas;

pub struct OverviewScreen {
    bardo: Entity<Bardo>,
    view: Option<OverviewView>,
    load_failed: bool,
}

impl EventEmitter<OpenVideo> for OverviewScreen {}
impl EventEmitter<OpenIdeas> for OverviewScreen {}

impl OverviewScreen {
    pub fn new(bardo: Entity<Bardo>, cx: &mut Context<Self>) -> Self {
        let mut screen = Self {
            bardo,
            view: None,
            load_failed: false,
        };
        screen.reload(cx);
        screen
    }

    /// Re-reads the open project: its videos moved on elsewhere.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        match self.bardo.read(cx).overview() {
            Ok(view) => {
                self.view = Some(view);
                self.load_failed = false;
            }
            Err(_) => {
                self.view = None;
                self.load_failed = true;
            }
        }
        cx.notify();
    }

    /// Whether the tour has something to show: a project with a channel.
    pub fn has_content(&self) -> bool {
        self.view
            .as_ref()
            .is_some_and(|view| !view.channels.is_empty())
    }

    fn videos(&self, view: &OverviewView, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let t = look(cx).tokens;
        let heading = kit::section_heading(tr(bardo, Text::OverviewPickUp));
        if view.videos.is_empty() {
            return kit::anchor(
                TourAnchor::Control(Control::OverviewVideos),
                v_flex().gap_2().child(heading).child(
                    div()
                        .text_sm()
                        .text_color(t.text2)
                        .child(tr(bardo, Text::OverviewNoVideos)),
                ),
            )
            .into_any_element();
        }
        let mut cards = Vec::new();
        for (ix, video) in view.videos.iter().enumerate() {
            let bardo = self.bardo.read(cx);
            let format = tr(bardo, Text::VideoFormatName(video.format));
            let mut chips = vec![
                kit::status(Tone::Neutral, video.channel_name.clone(), cx).into_any_element(),
                kit::status(Tone::Neutral, format, cx).into_any_element(),
            ];
            if video.is_ad {
                chips.push(
                    kit::status(Tone::Info, tr(bardo, Text::OverviewAd), cx).into_any_element(),
                );
            }
            let stage_line = video.status.map(|status| {
                SharedString::from(format!(
                    "{}: {}",
                    bardo.text(Text::StageName(status.stage)),
                    bardo.stage_note(status.note)
                ))
            });
            let next_line = SharedString::from(bardo.next_step_text(&video.next));
            let to_do = matches!(video.next, NextStep::Do(_));
            let stage = video.next.stage();
            let id = video.id;
            let button = if to_do {
                Button::new(SharedString::from(format!("overview-continue-{ix}")))
                    .primary()
                    .small()
                    .label(tr(bardo, Text::OverviewContinue))
            } else {
                Button::new(SharedString::from(format!("overview-open-{ix}")))
                    .small()
                    .label(tr(bardo, Text::OverviewOpen))
            }
            .on_click(cx.listener(move |_, _, _, cx| {
                cx.emit(OpenVideo { video: id, stage });
            }));
            let next = h_flex()
                .gap_3()
                .items_center()
                .flex_wrap()
                .child(
                    div()
                        .flex_1()
                        .min_w(gpui_kit::px(200.))
                        .text_sm()
                        .text_color(if to_do { t.accent_text } else { t.text2 })
                        .child(next_line),
                )
                .child(button);
            // The first card's step is what the tour lights.
            let next = if ix == 0 {
                kit::anchor(TourAnchor::Control(Control::OverviewNextStep), next).into_any_element()
            } else {
                next.into_any_element()
            };
            cards.push(
                kit::card(cx)
                    .p_3()
                    .gap_2()
                    .child(
                        div()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child(SharedString::from(video.title.clone())),
                    )
                    .child(h_flex().gap_1p5().flex_wrap().children(chips))
                    .children(
                        stage_line.map(|line| div().text_xs().text_color(t.text2).child(line)),
                    )
                    .child(next)
                    .into_any_element(),
            );
        }
        kit::anchor(
            TourAnchor::Control(Control::OverviewVideos),
            v_flex().gap_2().child(heading).children(cards),
        )
        .into_any_element()
    }

    fn ideas(&self, view: &OverviewView, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let t = look(cx).tokens;
        let n = |text: Text, n: usize| {
            SharedString::from(bardo.text_with(text, &[("n", &n.to_string())]))
        };
        let ideas = view.ideas;
        let lines: Vec<SharedString> = if ideas.is_empty() {
            vec![tr(bardo, Text::OverviewIdeasNone)]
        } else {
            [
                (ideas.unranked, Text::OverviewIdeasUnranked),
                (ideas.ranked, Text::OverviewIdeasRanked),
                (ideas.approved_without_video, Text::OverviewIdeasOrphans),
            ]
            .into_iter()
            .filter(|(count, _)| *count > 0)
            .map(|(count, text)| n(text, count))
            .collect()
        };
        kit::anchor(
            TourAnchor::Control(Control::OverviewIdeas),
            kit::card(cx)
                .p_3()
                .gap_2()
                .child(kit::section_heading(tr(bardo, Text::OverviewIdeasTitle)))
                .children(
                    lines
                        .into_iter()
                        .map(|line| div().text_sm().text_color(t.text).child(line)),
                )
                .child(
                    h_flex().child(
                        Button::new("overview-open-ideas")
                            .small()
                            .label(tr(bardo, Text::OverviewOpenIdeas))
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(OpenIdeas))),
                    ),
                ),
        )
        .into_any_element()
    }

    /// The project beside its videos: channels, this month's spending and
    /// the default narrators.
    fn project(&self, view: &OverviewView, cx: &App) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let t = look(cx).tokens;
        let label = |text: Text| {
            div()
                .text_xs()
                .text_color(t.text2)
                .font_weight(gpui_kit::FontWeight::MEDIUM)
                .child(tr(bardo, text))
        };
        let channels = view.channels.iter().map(|channel| {
            let place = format!(
                "{} · {}",
                bardo.text(Text::ContentLanguageName(channel.details.language())),
                bardo.text(Text::CountryName(channel.details.country()))
            );
            v_flex()
                .child(
                    div()
                        .text_sm()
                        .child(SharedString::from(channel.details.name().to_owned())),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(t.text2)
                        .child(SharedString::from(place)),
                )
        });
        let narrators: Vec<AnyElement> = if view.narrators.is_empty() {
            vec![
                div()
                    .text_sm()
                    .text_color(t.text2)
                    .child(tr(bardo, Text::OverviewNoNarrator))
                    .into_any_element(),
            ]
        } else {
            view.narrators
                .iter()
                .map(|name| {
                    div()
                        .text_sm()
                        .child(SharedString::from(name.clone()))
                        .into_any_element()
                })
                .collect()
        };
        kit::anchor(
            TourAnchor::Control(Control::OverviewProject),
            kit::card(cx)
                .p_3()
                .gap_3()
                .child(
                    v_flex()
                        .gap_1()
                        .child(label(Text::OverviewChannels))
                        .children(channels),
                )
                .child(
                    v_flex().gap_1().child(label(Text::OverviewSpent)).child(
                        div()
                            .text_lg()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child(SharedString::from(bardo.money(view.spent))),
                    ),
                )
                .child(
                    v_flex()
                        .gap_1()
                        .child(label(Text::OverviewNarrators))
                        .children(narrators),
                ),
        )
        .into_any_element()
    }
}

impl Render for OverviewScreen {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = self.view.clone();
        let (content, aside) = match &view {
            None => (
                vec![
                    kit::notice(
                        Tone::Danger,
                        tr(self.bardo.read(cx), Text::OverviewNotLoaded),
                        cx,
                    )
                    .into_any_element(),
                ],
                Vec::new(),
            ),
            Some(view) if view.channels.is_empty() => (
                vec![
                    div()
                        .text_sm()
                        .text_color(look(cx).tokens.text2)
                        .child(tr(self.bardo.read(cx), Text::OverviewNoProject))
                        .into_any_element(),
                ],
                Vec::new(),
            ),
            Some(view) => (
                vec![self.videos(view, cx), self.ideas(view, cx)],
                vec![self.project(view, cx)],
            ),
        };
        let has_content = self.has_content();
        let bardo = self.bardo.read(cx);
        let mut header = Header::place(bardo, Destination::Overview, cx);
        header.meta = Some(tr(bardo, Text::OverviewHint));
        header.info = guide::header_info(bardo, Destination::Overview, has_content, None, cx);
        let mut parts = ScreenParts::new(header);
        parts.content = content;
        parts.aside = aside;
        layout::screen(parts, cx)
    }
}
