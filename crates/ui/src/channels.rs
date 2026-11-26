//! Channels screen: the channels as a collection, the create/edit form as
//! its inspector.
//! Rules and persistence live in `bardo_app`; this file only maps the form to
//! a `ChannelDraft` and errors back to fields.

use bardo_app::bardo_domain::{
    CaptionHighlight, CaptionStyle, Channel, ChannelDraft, ChannelFieldError, ChannelId, ClipFit,
    ClipModelRef, ContentLanguage, Country, ModelName, MotionDefault, MotionPattern,
    MotionStrength, PersonaId, ProductionMode, StretchLimit, StyleChoice, TakeModel, ThumbnailLook,
    Transition, TransitionKind, VideoFormat, frame_time,
};
use std::rc::Rc;

use bardo_app::{Bardo, ChannelError, Control, Destination, GuideRef, Text, TourAnchor};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::searchable_list::{SearchableListItem, SearchableVec};
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::{ActiveTheme as _, IndexPath, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Entity, ScrollHandle, SharedString, Subscription, Window, div, px,
};

use crate::kit::{self, Tone};
use crate::model_picker::{ModelPicker, OnModel};
use crate::parts::{Collection, CollectionKind, Header, Inspector, ScreenParts, Tile};
use crate::shell::tr;
use crate::{guide, layout};

/// One option of a select: a domain value and its translated name.
#[derive(Clone)]
struct Choice<T> {
    value: T,
    title: SharedString,
}

impl<T: Clone + PartialEq> SearchableListItem for Choice<T> {
    type Value = T;

    fn title(&self) -> SharedString {
        self.title.clone()
    }

    fn value(&self) -> &T {
        &self.value
    }
}

type ChoiceSelect<T> = Entity<SelectState<SearchableVec<Choice<T>>>>;

fn language_choices(bardo: &Bardo) -> SearchableVec<Choice<ContentLanguage>> {
    SearchableVec::new(ContentLanguage::ALL.map(|value| Choice {
        value,
        title: tr(bardo, Text::ContentLanguageName(value)),
    }))
}

fn country_choices(bardo: &Bardo) -> SearchableVec<Choice<Country>> {
    SearchableVec::new(Country::ALL.map(|value| Choice {
        value,
        title: tr(bardo, Text::CountryName(value)),
    }))
}

/// "No default persona" first, then the profile's personas by name. A
/// list that cannot be read offers only "none".
fn persona_choices(bardo: &Bardo) -> SearchableVec<Choice<Option<PersonaId>>> {
    let none = Choice {
        value: None,
        title: tr(bardo, Text::ChannelPersonaNone),
    };
    let personas = bardo.personas().unwrap_or_default();
    SearchableVec::new(
        std::iter::once(none)
            .chain(personas.into_iter().map(|persona| Choice {
                value: Some(persona.id),
                title: SharedString::from(persona.details.name().to_owned()),
            }))
            .collect::<Vec<_>>(),
    )
}

/// A take model by name and provider: Seedance 2.5 comes through
/// Higgsfield and from BytePlus.
pub(crate) fn take_model_title(bardo: &Bardo, model: &TakeModel) -> String {
    format!(
        "{} · {}",
        model.name,
        bardo.text(Text::ProviderName(model.id.provider()))
    )
}

fn caption_style_choices(bardo: &Bardo) -> SearchableVec<Choice<CaptionStyle>> {
    SearchableVec::new(CaptionStyle::ALL.map(|value| Choice {
        value,
        title: tr(bardo, Text::CaptionStyleName(value)),
    }))
}

fn caption_highlight_choices(bardo: &Bardo) -> SearchableVec<Choice<CaptionHighlight>> {
    SearchableVec::new(CaptionHighlight::ALL.map(|value| Choice {
        value,
        title: tr(bardo, Text::CaptionHighlightName(value)),
    }))
}

fn thumbnail_look_choices(bardo: &Bardo) -> SearchableVec<Choice<ThumbnailLook>> {
    SearchableVec::new(ThumbnailLook::ALL.map(|value| Choice {
        value,
        title: tr(bardo, Text::ThumbnailLookName(value)),
    }))
}

fn format_choices(bardo: &Bardo) -> SearchableVec<Choice<VideoFormat>> {
    SearchableVec::new(VideoFormat::ALL.map(|value| Choice {
        value,
        title: tr(bardo, Text::VideoFormatName(value)),
    }))
}

fn production_choices(bardo: &Bardo) -> SearchableVec<Choice<ProductionMode>> {
    SearchableVec::new(ProductionMode::ALL.map(|value| Choice {
        value,
        title: tr(bardo, Text::ProductionModeName(value)),
    }))
}

fn clip_fit_choices(bardo: &Bardo) -> SearchableVec<Choice<ClipFit>> {
    SearchableVec::new(ClipFit::ALL.map(|value| Choice {
        value,
        title: tr(bardo, Text::ClipFitName(value)),
    }))
}

/// Each limit with what it means for a 10 second clip.
fn stretch_limit_choices(bardo: &Bardo) -> SearchableVec<Choice<StretchLimit>> {
    SearchableVec::new(
        StretchLimit::choices()
            .map(|value| Choice {
                value,
                title: SharedString::from(bardo.text_with(
                    Text::StretchLimitChoice,
                    &[
                        ("percent", &value.percent().to_string()),
                        ("seconds", &bardo.seconds(value.longest(10))),
                    ],
                )),
            })
            .collect::<Vec<_>>(),
    )
}

/// The channel's notes first, then the catalog in its own order.
fn style_choices(bardo: &Bardo) -> SearchableVec<Choice<StyleChoice>> {
    SearchableVec::new(
        StyleChoice::all()
            .map(|value| Choice {
                value,
                title: tr(bardo, Text::VisualStyleName(value)),
            })
            .collect::<Vec<_>>(),
    )
}

fn transition_choices(bardo: &Bardo) -> SearchableVec<Choice<TransitionKind>> {
    SearchableVec::new(TransitionKind::ALL.map(|value| Choice {
        value,
        title: tr(bardo, Text::TransitionName(value)),
    }))
}

fn motion_choices(bardo: &Bardo) -> SearchableVec<Choice<MotionPattern>> {
    SearchableVec::new(
        MotionPattern::all()
            .into_iter()
            .map(|value| Choice {
                value,
                title: tr(
                    bardo,
                    match value {
                        MotionPattern::None => Text::MotionNone,
                        MotionPattern::Varied => Text::MotionVaried,
                        MotionPattern::Same(kind) => Text::MotionName(kind),
                    },
                ),
            })
            .collect::<Vec<_>>(),
    )
}

fn motion_strength_choices(bardo: &Bardo) -> SearchableVec<Choice<MotionStrength>> {
    SearchableVec::new(MotionStrength::ALL.map(|value| Choice {
        value,
        title: tr(bardo, Text::MotionStrengthName(value)),
    }))
}

/// Every length a transition can be set to, in frames.
fn transition_lengths() -> Vec<u32> {
    (Transition::MIN_FRAMES..=Transition::MAX_FRAMES)
        .step_by(Transition::STEP_FRAMES as usize)
        .collect()
}

fn transition_length_choices(bardo: &Bardo) -> SearchableVec<Choice<u32>> {
    SearchableVec::new(
        transition_lengths()
            .into_iter()
            .map(|value| Choice {
                value,
                title: bardo.seconds(frame_time(u64::from(value))).into(),
            })
            .collect::<Vec<_>>(),
    )
}

fn index_of<T: PartialEq>(all: &[T], value: &T) -> Option<IndexPath> {
    all.iter().position(|v| v == value).map(IndexPath::new)
}

/// Field errors each input shows, and clears once the user edits it.
const NAME_ERRORS: &[ChannelFieldError] = &[
    ChannelFieldError::NameRequired,
    ChannelFieldError::NameTooLong,
];
const NICHE_ERRORS: &[ChannelFieldError] = &[ChannelFieldError::NicheTooLong];
const THEME_ERRORS: &[ChannelFieldError] = &[
    ChannelFieldError::TooManyThemes,
    ChannelFieldError::ThemeTooLong,
];
const AESTHETIC_NOTES_ERRORS: &[ChannelFieldError] = &[ChannelFieldError::AestheticNotesTooLong];

/// What the form footer says after the last action.
enum Notice {
    Saved,
    Error(Text),
}

pub struct ChannelsScreen {
    bardo: Entity<Bardo>,
    channels: Vec<Channel>,
    load_failed: bool,
    /// The channel being edited; `None` while creating a new one.
    editing: Option<ChannelId>,
    name: Entity<InputState>,
    niche: Entity<InputState>,
    themes: Entity<TextareaState>,
    aesthetic_notes: Entity<TextareaState>,
    language: ChoiceSelect<ContentLanguage>,
    country: ChoiceSelect<Country>,
    persona: ChoiceSelect<Option<PersonaId>>,
    /// The picked clip and take models; `None` follows the default.
    clip_model: Option<ClipModelRef>,
    take_model: Option<ClipModelRef>,
    caption_style: ChoiceSelect<CaptionStyle>,
    caption_highlight: ChoiceSelect<CaptionHighlight>,
    thumbnail_look: ChoiceSelect<ThumbnailLook>,
    format: ChoiceSelect<VideoFormat>,
    production: ChoiceSelect<ProductionMode>,
    style: ChoiceSelect<StyleChoice>,
    clip_fit: ChoiceSelect<ClipFit>,
    stretch_limit: ChoiceSelect<StretchLimit>,
    transition: ChoiceSelect<TransitionKind>,
    transition_length: ChoiceSelect<u32>,
    motion: ChoiceSelect<MotionPattern>,
    motion_strength: ChoiceSelect<MotionStrength>,
    field_errors: Vec<ChannelFieldError>,
    notice: Option<Notice>,
    /// The form's scroll, so the tour brings its fields into view.
    form_scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl ChannelsScreen {
    pub fn new(bardo: Entity<Bardo>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (channels, load_failed) = match bardo.read(cx).channels() {
            Ok(channels) => (channels, false),
            Err(_) => (Vec::new(), true),
        };

        let name = cx.new(|cx| InputState::new(window, cx));
        let niche = cx.new(|cx| InputState::new(window, cx));
        let themes = cx.new(|cx| TextareaState::new(window, cx).auto_grow(3, 10));
        let aesthetic_notes = cx.new(|cx| TextareaState::new(window, cx).auto_grow(3, 10));

        let defaults = ChannelDraft::default();
        let (languages, countries, personas, caption_styles) = {
            let bardo = bardo.read(cx);
            (
                language_choices(bardo),
                country_choices(bardo),
                persona_choices(bardo),
                caption_style_choices(bardo),
            )
        };
        let caption_highlights = caption_highlight_choices(bardo.read(cx));
        let thumbnail_looks = thumbnail_look_choices(bardo.read(cx));
        let (transitions, transition_lengths_list, formats, motions, strengths) = {
            let bardo = bardo.read(cx);
            (
                transition_choices(bardo),
                transition_length_choices(bardo),
                format_choices(bardo),
                motion_choices(bardo),
                motion_strength_choices(bardo),
            )
        };
        let language = cx.new(|cx| {
            let selected = index_of(&ContentLanguage::ALL, &defaults.language);
            SelectState::new(languages, selected, window, cx)
        });
        let country = cx.new(|cx| {
            let selected = index_of(&Country::ALL, &defaults.country);
            SelectState::new(countries, selected, window, cx).searchable(true)
        });
        let persona = cx.new(|cx| {
            SelectState::new(personas, Some(IndexPath::new(0)), window, cx).searchable(true)
        });
        let caption_style = cx.new(|cx| {
            let selected = index_of(&CaptionStyle::ALL, &defaults.caption_style);
            SelectState::new(caption_styles, selected, window, cx)
        });
        let caption_highlight = cx.new(|cx| {
            let selected = index_of(&CaptionHighlight::ALL, &defaults.caption_highlight);
            SelectState::new(caption_highlights, selected, window, cx)
        });
        let thumbnail_look = cx.new(|cx| {
            let selected = index_of(&ThumbnailLook::ALL, &defaults.thumbnail_look);
            SelectState::new(thumbnail_looks, selected, window, cx)
        });
        let format = cx.new(|cx| {
            let selected = index_of(&VideoFormat::ALL, &defaults.format);
            SelectState::new(formats, selected, window, cx)
        });
        let productions = production_choices(bardo.read(cx));
        let production = cx.new(|cx| {
            let selected = index_of(&ProductionMode::ALL, &defaults.production);
            SelectState::new(productions, selected, window, cx)
        });
        let styles = style_choices(bardo.read(cx));
        let style = cx.new(|cx| {
            let all: Vec<StyleChoice> = StyleChoice::all().collect();
            let selected = index_of(&all, &defaults.style);
            SelectState::new(styles, selected, window, cx).searchable(true)
        });
        let clip_fits = clip_fit_choices(bardo.read(cx));
        let clip_fit = cx.new(|cx| {
            let selected = index_of(&ClipFit::ALL, &defaults.clip_fit);
            SelectState::new(clip_fits, selected, window, cx)
        });
        let stretch_limits = stretch_limit_choices(bardo.read(cx));
        let stretch_limit = cx.new(|cx| {
            let all: Vec<StretchLimit> = StretchLimit::choices().collect();
            let selected = index_of(&all, &defaults.stretch_limit);
            SelectState::new(stretch_limits, selected, window, cx)
        });
        let transition = cx.new(|cx| {
            let selected = index_of(&TransitionKind::ALL, &defaults.transition.kind);
            SelectState::new(transitions, selected, window, cx)
        });
        let transition_length = cx.new(|cx| {
            let selected = index_of(&transition_lengths(), &defaults.transition.frames());
            SelectState::new(transition_lengths_list, selected, window, cx)
        });
        let motion = cx.new(|cx| {
            let selected = index_of(&MotionPattern::all(), &defaults.motion.pattern);
            SelectState::new(motions, selected, window, cx)
        });
        let motion_strength = cx.new(|cx| {
            let selected = index_of(&MotionStrength::ALL, &defaults.motion.strength);
            SelectState::new(strengths, selected, window, cx)
        });

        let subscriptions = vec![
            cx.subscribe(&name, |this, _, event, cx| {
                this.edited(NAME_ERRORS, event, cx)
            }),
            cx.subscribe(&niche, |this, _, event, cx| {
                this.edited(NICHE_ERRORS, event, cx)
            }),
            cx.subscribe(&themes, |this, _, event, cx| {
                this.edited(THEME_ERRORS, event, cx)
            }),
            cx.subscribe(&aesthetic_notes, |this, _, event, cx| {
                this.edited(AESTHETIC_NOTES_ERRORS, event, cx)
            }),
            cx.observe_in(&bardo, window, |this, _, window, cx| {
                this.relabel(window, cx)
            }),
            // The line under the mode says what the picked one makes.
            cx.subscribe(
                &production,
                |_, _, _: &SelectEvent<SearchableVec<Choice<ProductionMode>>>, cx| cx.notify(),
            ),
            // The limit shows only while clips are stretched.
            cx.subscribe(
                &clip_fit,
                |_, _, _: &SelectEvent<SearchableVec<Choice<ClipFit>>>, cx| cx.notify(),
            ),
        ];

        let mut screen = Self {
            bardo,
            channels,
            load_failed,
            editing: None,
            name,
            niche,
            themes,
            aesthetic_notes,
            language,
            country,
            persona,
            clip_model: None,
            take_model: None,
            caption_style,
            caption_highlight,
            thumbnail_look,
            format,
            production,
            style,
            clip_fit,
            stretch_limit,
            transition,
            transition_length,
            motion,
            motion_strength,
            field_errors: Vec::new(),
            notice: None,
            form_scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        };
        screen.relabel(window, cx);
        screen
    }

    /// Editing a field hides its now-stale error, and the "saved" notice so
    /// it never describes unsaved edits.
    fn edited(&mut self, fields: &[ChannelFieldError], event: &InputEvent, cx: &mut Context<Self>) {
        if !matches!(event, InputEvent::Change) {
            return;
        }
        self.field_errors.retain(|error| !fields.contains(error));
        if matches!(self.notice, Some(Notice::Saved)) {
            self.notice = None;
        }
        cx.notify();
    }

    /// Re-applies every translated label the input states hold, after the
    /// interface language changes. Selections are kept.
    fn relabel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let bardo = self.bardo.read(cx);
        let name = tr(bardo, Text::ChannelNamePlaceholder);
        let niche = tr(bardo, Text::ChannelNichePlaceholder);
        let themes = tr(bardo, Text::ChannelThemesPlaceholder);
        let aesthetic = tr(bardo, Text::ChannelAestheticNotesPlaceholder);
        let languages = language_choices(bardo);
        let countries = country_choices(bardo);
        let personas = persona_choices(bardo);
        let caption_styles = caption_style_choices(bardo);
        let caption_highlights = caption_highlight_choices(bardo);
        let thumbnail_looks = thumbnail_look_choices(bardo);
        let transitions = transition_choices(bardo);
        let lengths = transition_length_choices(bardo);
        let formats = format_choices(bardo);
        let productions = production_choices(bardo);
        let styles = style_choices(bardo);
        let clip_fits = clip_fit_choices(bardo);
        let stretch_limits = stretch_limit_choices(bardo);
        let motions = motion_choices(bardo);
        let strengths = motion_strength_choices(bardo);

        self.name
            .update(cx, |input, cx| input.set_placeholder(name, window, cx));
        self.niche
            .update(cx, |input, cx| input.set_placeholder(niche, window, cx));
        self.themes
            .update(cx, |input, cx| input.set_placeholder(themes, window, cx));
        self.aesthetic_notes
            .update(cx, |input, cx| input.set_placeholder(aesthetic, window, cx));
        self.language.update(cx, |select, cx| {
            let selected = select.selected_value().copied();
            select.set_items(languages, window, cx);
            if let Some(value) = selected {
                select.set_selected_value(&value, window, cx);
            }
        });
        self.country.update(cx, |select, cx| {
            let selected = select.selected_value().copied();
            select.set_items(countries, window, cx);
            if let Some(value) = selected {
                select.set_selected_value(&value, window, cx);
            }
        });
        Self::set_persona_items(&self.persona, personas, window, cx);
        self.caption_style.update(cx, |select, cx| {
            let selected = select.selected_value().copied();
            select.set_items(caption_styles, window, cx);
            if let Some(value) = selected {
                select.set_selected_value(&value, window, cx);
            }
        });
        self.caption_highlight.update(cx, |select, cx| {
            let selected = select.selected_value().copied();
            select.set_items(caption_highlights, window, cx);
            if let Some(value) = selected {
                select.set_selected_value(&value, window, cx);
            }
        });
        self.thumbnail_look.update(cx, |select, cx| {
            let selected = select.selected_value().copied();
            select.set_items(thumbnail_looks, window, cx);
            if let Some(value) = selected {
                select.set_selected_value(&value, window, cx);
            }
        });
        self.format.update(cx, |select, cx| {
            let selected = select.selected_value().copied();
            select.set_items(formats, window, cx);
            if let Some(value) = selected {
                select.set_selected_value(&value, window, cx);
            }
        });
        self.production.update(cx, |select, cx| {
            let selected = select.selected_value().copied();
            select.set_items(productions, window, cx);
            if let Some(value) = selected {
                select.set_selected_value(&value, window, cx);
            }
        });
        self.style.update(cx, |select, cx| {
            let selected = select.selected_value().copied();
            select.set_items(styles, window, cx);
            if let Some(value) = selected {
                select.set_selected_value(&value, window, cx);
            }
        });
        self.clip_fit.update(cx, |select, cx| {
            let selected = select.selected_value().copied();
            select.set_items(clip_fits, window, cx);
            if let Some(value) = selected {
                select.set_selected_value(&value, window, cx);
            }
        });
        self.stretch_limit.update(cx, |select, cx| {
            let selected = select.selected_value().copied();
            select.set_items(stretch_limits, window, cx);
            if let Some(value) = selected {
                select.set_selected_value(&value, window, cx);
            }
        });
        self.transition.update(cx, |select, cx| {
            let selected = select.selected_value().copied();
            select.set_items(transitions, window, cx);
            if let Some(value) = selected {
                select.set_selected_value(&value, window, cx);
            }
        });
        self.transition_length.update(cx, |select, cx| {
            let selected = select.selected_value().copied();
            select.set_items(lengths, window, cx);
            if let Some(value) = selected {
                select.set_selected_value(&value, window, cx);
            }
        });
        self.motion.update(cx, |select, cx| {
            let selected = select.selected_value().copied();
            select.set_items(motions, window, cx);
            if let Some(value) = selected {
                select.set_selected_value(&value, window, cx);
            }
        });
        self.motion_strength.update(cx, |select, cx| {
            let selected = select.selected_value().copied();
            select.set_items(strengths, window, cx);
            if let Some(value) = selected {
                select.set_selected_value(&value, window, cx);
            }
        });
        // A model no longer offered (its provider's key removed meanwhile)
        // goes back to the default, as a select dropped it before (#329).
        let bardo = self.bardo.read(cx);
        let (clips, takes) = (bardo.clip_models(), bardo.take_models());
        self.clip_model = self
            .clip_model
            .take()
            .filter(|model| clips.iter().any(|offered| offered.id == *model));
        self.take_model = self
            .take_model
            .take()
            .filter(|model| takes.iter().any(|offered| offered.id == *model));
        cx.notify();
    }

    /// Re-reads the persona choices; personas may have been created or
    /// renamed on the personas screen.
    pub fn reload_personas(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let personas = persona_choices(self.bardo.read(cx));
        Self::set_persona_items(&self.persona, personas, window, cx);
        cx.notify();
    }

    /// Replaces the choices and keeps the selection, falling back to "none"
    /// when the selected persona is gone.
    fn set_persona_items(
        select: &ChoiceSelect<Option<PersonaId>>,
        personas: SearchableVec<Choice<Option<PersonaId>>>,
        window: &mut Window,
        cx: &mut App,
    ) {
        select.update(cx, |select, cx| {
            let selected = select.selected_value().copied().flatten();
            select.set_items(personas, window, cx);
            select.set_selected_value(&selected, window, cx);
            if select.selected_value().is_none() {
                select.set_selected_value(&None, window, cx);
            }
        });
    }

    fn fill(&mut self, draft: &ChannelDraft, window: &mut Window, cx: &mut Context<Self>) {
        let themes = draft.themes.join("\n");
        self.name.update(cx, |input, cx| {
            input.set_value(draft.name.clone(), window, cx)
        });
        self.niche.update(cx, |input, cx| {
            input.set_value(draft.niche.clone(), window, cx)
        });
        self.themes
            .update(cx, |input, cx| input.set_value(themes, window, cx));
        self.aesthetic_notes.update(cx, |input, cx| {
            input.set_value(draft.aesthetic_notes.clone(), window, cx)
        });
        self.language.update(cx, |select, cx| {
            select.set_selected_value(&draft.language, window, cx)
        });
        self.country.update(cx, |select, cx| {
            select.set_selected_value(&draft.country, window, cx)
        });
        self.persona.update(cx, |select, cx| {
            select.set_selected_value(&draft.default_persona, window, cx)
        });
        // A model its provider retired shows as the one that took its
        // place; a model no longer offered shows as the default, and saving
        // keeps the form's choice.
        let bardo = self.bardo.read(cx);
        self.clip_model = draft
            .clip_model
            .as_ref()
            .map(|model| bardo.clip_model_now(model))
            .filter(|model| {
                bardo
                    .clip_models()
                    .iter()
                    .any(|offered| offered.id == *model)
            });
        self.take_model = draft.take_model.clone().filter(|model| {
            bardo
                .take_models()
                .iter()
                .any(|offered| offered.id == *model)
        });
        self.caption_style.update(cx, |select, cx| {
            select.set_selected_value(&draft.caption_style, window, cx)
        });
        self.caption_highlight.update(cx, |select, cx| {
            select.set_selected_value(&draft.caption_highlight, window, cx)
        });
        self.thumbnail_look.update(cx, |select, cx| {
            select.set_selected_value(&draft.thumbnail_look, window, cx)
        });
        self.format.update(cx, |select, cx| {
            select.set_selected_value(&draft.format, window, cx)
        });
        self.production.update(cx, |select, cx| {
            select.set_selected_value(&draft.production, window, cx)
        });
        self.style.update(cx, |select, cx| {
            select.set_selected_value(&draft.style, window, cx)
        });
        self.clip_fit.update(cx, |select, cx| {
            select.set_selected_value(&draft.clip_fit, window, cx)
        });
        // A limit between two of the menu's (from a settings file) shows
        // as the nearest one.
        let limit = StretchLimit::choices()
            .min_by_key(|choice| choice.percent().abs_diff(draft.stretch_limit.percent()))
            .unwrap_or_default();
        self.stretch_limit.update(cx, |select, cx| {
            select.set_selected_value(&limit, window, cx)
        });
        self.transition.update(cx, |select, cx| {
            select.set_selected_value(&draft.transition.kind, window, cx)
        });
        self.transition_length.update(cx, |select, cx| {
            select.set_selected_value(&draft.transition.frames(), window, cx)
        });
        self.motion.update(cx, |select, cx| {
            select.set_selected_value(&draft.motion.pattern, window, cx)
        });
        self.motion_strength.update(cx, |select, cx| {
            select.set_selected_value(&draft.motion.strength, window, cx)
        });
    }

    fn draft(&self, cx: &App) -> ChannelDraft {
        ChannelDraft {
            name: self.name.read(cx).value().to_string(),
            niche: self.niche.read(cx).value().to_string(),
            themes: self
                .themes
                .read(cx)
                .value()
                .lines()
                .map(str::to_owned)
                .collect(),
            aesthetic_notes: self.aesthetic_notes.read(cx).value().to_string(),
            language: self
                .language
                .read(cx)
                .selected_value()
                .copied()
                .unwrap_or_default(),
            country: self
                .country
                .read(cx)
                .selected_value()
                .copied()
                .unwrap_or_default(),
            default_persona: self.persona.read(cx).selected_value().copied().flatten(),
            clip_model: self.clip_model.clone(),
            take_model: self.take_model.clone(),
            caption_style: self
                .caption_style
                .read(cx)
                .selected_value()
                .copied()
                .unwrap_or_default(),
            caption_highlight: self
                .caption_highlight
                .read(cx)
                .selected_value()
                .copied()
                .unwrap_or_default(),
            thumbnail_look: self
                .thumbnail_look
                .read(cx)
                .selected_value()
                .copied()
                .unwrap_or_default(),
            format: self
                .format
                .read(cx)
                .selected_value()
                .copied()
                .unwrap_or_default(),
            transition: Transition::new(
                self.transition
                    .read(cx)
                    .selected_value()
                    .copied()
                    .unwrap_or_default(),
                self.transition_length
                    .read(cx)
                    .selected_value()
                    .copied()
                    .unwrap_or(Transition::DEFAULT_FRAMES),
            ),
            motion: MotionDefault::new(
                self.motion
                    .read(cx)
                    .selected_value()
                    .copied()
                    .unwrap_or_default(),
                self.motion_strength
                    .read(cx)
                    .selected_value()
                    .copied()
                    .unwrap_or_default(),
            ),
            production: self
                .production
                .read(cx)
                .selected_value()
                .copied()
                .unwrap_or_default(),
            style: self
                .style
                .read(cx)
                .selected_value()
                .copied()
                .unwrap_or_default(),
            clip_fit: self
                .clip_fit
                .read(cx)
                .selected_value()
                .copied()
                .unwrap_or_default(),
            stretch_limit: self
                .stretch_limit
                .read(cx)
                .selected_value()
                .copied()
                .unwrap_or_default(),
        }
    }

    fn start_new(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = None;
        self.field_errors.clear();
        self.notice = None;
        self.fill(&ChannelDraft::default(), window, cx);
        cx.notify();
    }

    fn edit(&mut self, id: ChannelId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(channel) = self.channels.iter().find(|c| c.id == id) else {
            return;
        };
        let draft = ChannelDraft::from(&channel.details);
        self.editing = Some(id);
        self.field_errors.clear();
        self.notice = None;
        self.fill(&draft, window, cx);
        cx.notify();
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let draft = self.draft(cx);
        let bardo = self.bardo.read(cx);
        let result = match self.editing {
            Some(id) => bardo.update_channel(id, draft),
            None => bardo.create_channel(draft),
        };
        match result {
            Ok(saved) => {
                // The list is re-read so it shows the stored order and values.
                match self.bardo.read(cx).channels() {
                    Ok(channels) => {
                        self.channels = channels;
                        self.load_failed = false;
                    }
                    Err(_) => self.load_failed = true,
                }
                self.editing = Some(saved.id);
                self.field_errors.clear();
                // Show the normalized values (trimmed, deduplicated themes).
                self.fill(&ChannelDraft::from(&saved.details), window, cx);
                self.notice = Some(Notice::Saved);
            }
            Err(error) => self.show_error(&error),
        }
        cx.notify();
    }

    /// Whether there are channels to show: the screen offers its tour only
    /// then.
    pub fn has_content(&self) -> bool {
        !self.load_failed && !self.channels.is_empty()
    }

    fn show_error(&mut self, error: &ChannelError) {
        self.field_errors = error.field_errors().to_vec();
        self.notice = error.form_message().map(Notice::Error);
    }

    fn collection(&self, cx: &mut Context<Self>) -> Collection {
        let mut collection = Collection::new(CollectionKind::List, "channel-list");
        let tiles: Vec<Tile> = self
            .channels
            .iter()
            .enumerate()
            .map(|(ix, channel)| {
                let id = channel.id;
                let details = &channel.details;
                let bardo = self.bardo.read(cx);
                let mut summary = vec![tr(bardo, Text::CountryName(details.country())).to_string()];
                if !details.niche().is_empty() {
                    summary.insert(0, details.niche().to_owned());
                }
                let mut tile = Tile::new(
                    ("channel", ix),
                    Rc::new(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.edit(id, window, cx)
                    })),
                );
                tile.selected = self.editing == Some(id);
                tile.title = Some(SharedString::from(details.name().to_owned()));
                tile.text = Some(SharedString::from(summary.join(" · ")));
                tile
            })
            .collect();
        let bardo = self.bardo.read(cx);
        collection.empty = Some(if self.load_failed {
            kit::notice(Tone::Danger, tr(bardo, Text::ChannelsNotLoaded), cx).into_any_element()
        } else {
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(tr(bardo, Text::ChannelsEmpty))
                .into_any_element()
        });
        if !self.load_failed {
            collection.tiles = tiles;
        }
        collection
    }

    /// The clip and take model pickers (#329): provider, model, version
    /// and variant, the default first.
    fn model_pickers(&self, cx: &mut Context<Self>) -> (AnyElement, AnyElement) {
        let screen = cx.entity().downgrade();
        let pick = |take: bool| -> OnModel {
            let screen = screen.clone();
            Rc::new(move |model, _, cx| {
                let _ = screen.update(cx, |this, cx| {
                    if take {
                        this.take_model = model;
                    } else {
                        this.clip_model = model;
                    }
                    if matches!(this.notice, Some(Notice::Saved)) {
                        this.notice = None;
                    }
                    cx.notify();
                });
            })
        };
        let bardo = self.bardo.read(cx);
        let clips: Vec<(ClipModelRef, ModelName)> = bardo
            .clip_models()
            .into_iter()
            .map(|model| (model.id, model.name))
            .collect();
        let takes: Vec<(ClipModelRef, ModelName)> = bardo
            .take_models()
            .into_iter()
            .map(|model| (model.id, model.name))
            .collect();
        let picker = |id: &'static str,
                      models: Vec<(ClipModelRef, ModelName)>,
                      value: Option<ClipModelRef>,
                      on_pick: OnModel| {
            // The first model is the provider's default.
            let default = models.first().map(|(id, _)| id.clone());
            ModelPicker::new(id, models, value, on_pick)
                .follow(Text::ChannelClipModelDefault, default)
                .render(bardo, cx)
        };
        (
            picker(
                "channel-clip-model",
                clips,
                self.clip_model.clone(),
                pick(false),
            ),
            picker(
                "channel-take-model",
                takes,
                self.take_model.clone(),
                pick(true),
            ),
        )
    }

    fn render_form(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (clip_picker, take_picker) = self.model_pickers(cx);
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let error_for = |fields: &[ChannelFieldError]| -> Option<AnyElement> {
            let error = self.field_errors.iter().find(|e| fields.contains(e))?;
            Some(
                div()
                    .text_xs()
                    .text_color(theme.danger)
                    .child(tr(bardo, Text::ChannelFieldError(*error)))
                    .into_any_element(),
            )
        };
        let field = |label: Text, control: AnyElement, below: Option<AnyElement>| {
            kit::field(tr(bardo, label), None, control, below)
        };
        // Explanations go behind an ⓘ beside the label.
        let explained =
            |id: &'static str, label: Text, hint: Text, section: GuideRef, control: AnyElement| {
                kit::field(
                    tr(bardo, label),
                    Some(guide::info(bardo, id, tr(bardo, hint), section)),
                    control,
                    None,
                )
            };
        let scroll = Some(&self.form_scroll);
        let tagged =
            |anchor: TourAnchor, element: AnyElement| kit::anchor_in(anchor, element, scroll);
        let hint = |text: Text| {
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(tr(bardo, text))
                .into_any_element()
        };

        let action = match self.editing {
            Some(_) => Text::SaveChannel,
            None => Text::CreateChannel,
        };
        let notice = self.notice.as_ref().map(|notice| match notice {
            Notice::Saved => kit::notice(Tone::Success, tr(bardo, Text::ChannelSaved), cx),
            Notice::Error(text) => kit::notice(Tone::Danger, tr(bardo, *text), cx),
        });

        v_flex().max_w(px(640.)).child(
            kit::card(cx)
                .p_4()
                .gap_4()
                .child(field(
                    Text::ChannelName,
                    Input::new(&self.name).into_any_element(),
                    error_for(NAME_ERRORS),
                ))
                .child(tagged(
                    TourAnchor::Control(Control::ChannelNiche),
                    v_flex()
                        .gap_4()
                        .child(field(
                            Text::ChannelNiche,
                            Input::new(&self.niche).into_any_element(),
                            error_for(NICHE_ERRORS),
                        ))
                        .child(field(
                            Text::ChannelThemes,
                            Textarea::new(&self.themes).into_any_element(),
                            error_for(THEME_ERRORS).or_else(|| Some(hint(Text::ChannelThemesHint))),
                        ))
                        .child(field(
                            Text::ChannelAestheticNotes,
                            Textarea::new(&self.aesthetic_notes).into_any_element(),
                            error_for(AESTHETIC_NOTES_ERRORS),
                        ))
                        .child(explained(
                            "channel-style-info",
                            Text::ChannelStyle,
                            Text::ChannelStyleHint,
                            guide::refs::CHANNELS_STYLE,
                            Select::new(&self.style)
                                .search_placeholder(tr(bardo, Text::ChannelStyle))
                                .into_any_element(),
                        ))
                        .into_any_element(),
                ))
                .child(tagged(
                    TourAnchor::Control(Control::ChannelMarket),
                    h_flex()
                        .gap_4()
                        .child(div().flex_1().child(field(
                            Text::ChannelLanguage,
                            Select::new(&self.language).into_any_element(),
                            None,
                        )))
                        .child(
                            div().flex_1().child(field(
                                Text::ChannelCountry,
                                Select::new(&self.country)
                                    .search_placeholder(tr(bardo, Text::ChannelCountrySearch))
                                    .into_any_element(),
                                None,
                            )),
                        )
                        .into_any_element(),
                ))
                .child(tagged(
                    TourAnchor::Control(Control::ChannelPersona),
                    explained(
                        "channel-persona-info",
                        Text::ChannelPersona,
                        Text::ChannelPersonaHint,
                        guide::refs::CHANNELS_PERSONA,
                        Select::new(&self.persona)
                            .search_placeholder(tr(bardo, Text::PersonasTitle))
                            .into_any_element(),
                    )
                    .into_any_element(),
                ))
                .child(tagged(
                    TourAnchor::Control(Control::ChannelFormat),
                    v_flex()
                        .gap_4()
                        .child(explained(
                            "channel-format-info",
                            Text::ChannelFormat,
                            Text::ChannelFormatHint,
                            guide::refs::CHANNELS_FORMAT,
                            Select::new(&self.format).into_any_element(),
                        ))
                        .child(explained(
                            "channel-production-info",
                            Text::ChannelProduction,
                            Text::ChannelProductionHint,
                            guide::refs::CHANNELS_PRODUCTION,
                            v_flex()
                                .gap_1()
                                .child(Select::new(&self.production))
                                .children(self.production.read(cx).selected_value().map(|mode| {
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(tr(bardo, Text::ProductionModeAbout(*mode)))
                                }))
                                .into_any_element(),
                        ))
                        .into_any_element(),
                ))
                .child(tagged(
                    TourAnchor::Control(Control::ChannelDefaults),
                    v_flex()
                        .gap_4()
                        .child(explained(
                            "channel-clip-model-info",
                            Text::ChannelClipModel,
                            Text::ChannelClipModelHint,
                            guide::refs::CHANNELS_DEFAULTS,
                            clip_picker,
                        ))
                        .child(explained(
                            "channel-clip-fit-info",
                            Text::ChannelClipFit,
                            Text::ChannelClipFitHint,
                            guide::refs::CHANNELS_CLIP_FIT,
                            Select::new(&self.clip_fit).into_any_element(),
                        ))
                        .when(
                            self.clip_fit.read(cx).selected_value() == Some(&ClipFit::Stretch),
                            |fields| {
                                fields.child(explained(
                                    "channel-stretch-limit-info",
                                    Text::ChannelStretchLimit,
                                    Text::ChannelStretchLimitHint,
                                    guide::refs::CHANNELS_CLIP_FIT,
                                    Select::new(&self.stretch_limit).into_any_element(),
                                ))
                            },
                        )
                        .child(explained(
                            "channel-take-model-info",
                            Text::ChannelTakeModel,
                            Text::ChannelTakeModelHint,
                            guide::refs::CHANNELS_DEFAULTS,
                            take_picker,
                        ))
                        .child(
                            h_flex()
                                .gap_4()
                                .items_start()
                                .child(div().flex_1().min_w_0().child(explained(
                                    "channel-caption-style-info",
                                    Text::ChannelCaptionStyle,
                                    Text::ChannelCaptionStyleHint,
                                    guide::refs::CHANNELS_DEFAULTS,
                                    Select::new(&self.caption_style).into_any_element(),
                                )))
                                .child(div().w(px(160.)).flex_none().child(explained(
                                    "channel-caption-highlight-info",
                                    Text::ChannelCaptionHighlight,
                                    Text::ChannelCaptionHighlightHint,
                                    guide::refs::CHANNELS_DEFAULTS,
                                    Select::new(&self.caption_highlight).into_any_element(),
                                ))),
                        )
                        .child(
                            h_flex()
                                .gap_4()
                                .items_start()
                                .child(div().flex_1().min_w_0().child(explained(
                                    "channel-transition-info",
                                    Text::ChannelTransition,
                                    Text::ChannelTransitionHint,
                                    guide::refs::CHANNELS_DEFAULTS,
                                    Select::new(&self.transition).into_any_element(),
                                )))
                                .child(div().w(px(112.)).flex_none().child(kit::field(
                                    tr(bardo, Text::ChannelTransitionLength),
                                    None,
                                    Select::new(&self.transition_length).into_any_element(),
                                    None,
                                ))),
                        )
                        .child(
                            h_flex()
                                .gap_4()
                                .items_start()
                                .child(div().flex_1().min_w_0().child(explained(
                                    "channel-motion-info",
                                    Text::ChannelMotion,
                                    Text::ChannelMotionHint,
                                    guide::refs::CHANNELS_DEFAULTS,
                                    Select::new(&self.motion).into_any_element(),
                                )))
                                .child(div().w(px(112.)).flex_none().child(kit::field(
                                    tr(bardo, Text::ChannelMotionStrength),
                                    None,
                                    Select::new(&self.motion_strength).into_any_element(),
                                    None,
                                ))),
                        )
                        .child(explained(
                            "channel-thumbnail-look-info",
                            Text::ChannelThumbnailLook,
                            Text::ChannelThumbnailLookHint,
                            guide::refs::CHANNELS_DEFAULTS,
                            Select::new(&self.thumbnail_look).into_any_element(),
                        ))
                        .into_any_element(),
                ))
                .child(
                    h_flex()
                        .gap_3()
                        .child(
                            Button::new("save-channel")
                                .primary()
                                .label(tr(bardo, action))
                                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                    this.save(window, cx)
                                })),
                        )
                        .children(notice),
                ),
        )
    }
}

impl Render for ChannelsScreen {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let collection = self.collection(cx);
        let form = self.render_form(cx).into_any_element();
        let bardo = self.bardo.read(cx);
        let mut header = Header::place(bardo, Destination::Channels);
        header.info =
            guide::header_info(bardo, Destination::Channels, self.has_content(), None, cx);
        header.actions.push(
            Button::new("new-channel")
                .small()
                .primary()
                .label(tr(bardo, Text::NewChannel))
                .on_click(
                    cx.listener(|this, _: &ClickEvent, window, cx| this.start_new(window, cx)),
                )
                .into_any_element(),
        );
        let title = match self.editing {
            Some(_) => Text::EditChannelTitle,
            None => Text::NewChannelTitle,
        };
        let mut parts = ScreenParts::new(header);
        parts.collection = Some(collection);
        parts.inspector = Some(Inspector {
            title: Some(kit::section_heading(tr(bardo, title)).into_any_element()),
            body: vec![form],
            media: None,
            footer: None,
            scroll: None,
        });
        parts.scroll = Some(self.form_scroll.clone());
        layout::screen(parts, cx)
    }
}
