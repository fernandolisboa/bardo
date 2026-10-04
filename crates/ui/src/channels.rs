//! Channels screen: the channels as a collection, the create/edit form as
//! its inspector.
//! Rules and persistence live in `bardo_app`; this file only maps the form to
//! a `ChannelDraft` and errors back to fields.

use bardo_app::bardo_domain::{
    CaptionStyle, Channel, ChannelDraft, ChannelFieldError, ChannelId, ClipModelRef,
    ContentLanguage, Country, PersonaId,
};
use std::rc::Rc;

use bardo_app::{Bardo, ChannelError, Destination, Text};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::searchable_list::{SearchableListItem, SearchableVec};
use gpui_kit::component::select::{Select, SelectState};
use gpui_kit::component::{ActiveTheme as _, IndexPath, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, ClickEvent, Entity, SharedString, Subscription, Window, div, px};

use crate::kit::{self, Tone};
use crate::layout;
use crate::parts::{Collection, CollectionKind, Header, Inspector, ScreenParts, Tile};
use crate::shell::tr;

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

/// The provider's default model first (a channel that keeps it follows the
/// default), then every model by name and provider.
fn clip_model_choices(bardo: &Bardo) -> SearchableVec<Choice<Option<ClipModelRef>>> {
    let models = bardo.clip_models();
    let default = models.first().map(|model| Choice {
        value: None,
        title: SharedString::from(
            bardo.text_with(Text::ChannelClipModelDefault, &[("model", &model.name)]),
        ),
    });
    SearchableVec::new(
        default
            .into_iter()
            .chain(models.into_iter().map(|model| Choice {
                title: SharedString::from(format!(
                    "{} · {}",
                    model.name,
                    bardo.text(Text::ProviderName(model.id.provider()))
                )),
                value: Some(model.id),
            }))
            .collect::<Vec<_>>(),
    )
}

fn caption_style_choices(bardo: &Bardo) -> SearchableVec<Choice<CaptionStyle>> {
    SearchableVec::new(CaptionStyle::ALL.map(|value| Choice {
        value,
        title: tr(bardo, Text::CaptionStyleName(value)),
    }))
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
    clip_model: ChoiceSelect<Option<ClipModelRef>>,
    caption_style: ChoiceSelect<CaptionStyle>,
    field_errors: Vec<ChannelFieldError>,
    notice: Option<Notice>,
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
        let (languages, countries, personas, clip_models, caption_styles) = {
            let bardo = bardo.read(cx);
            (
                language_choices(bardo),
                country_choices(bardo),
                persona_choices(bardo),
                clip_model_choices(bardo),
                caption_style_choices(bardo),
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
        let clip_model =
            cx.new(|cx| SelectState::new(clip_models, Some(IndexPath::new(0)), window, cx));
        let caption_style = cx.new(|cx| {
            let selected = index_of(&CaptionStyle::ALL, &defaults.caption_style);
            SelectState::new(caption_styles, selected, window, cx)
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
            clip_model,
            caption_style,
            field_errors: Vec::new(),
            notice: None,
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
        let clip_models = clip_model_choices(bardo);
        let caption_styles = caption_style_choices(bardo);

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
        self.clip_model.update(cx, |select, cx| {
            let selected = select.selected_value().cloned().flatten();
            select.set_items(clip_models, window, cx);
            select.set_selected_value(&selected, window, cx);
        });
        self.caption_style.update(cx, |select, cx| {
            let selected = select.selected_value().copied();
            select.set_items(caption_styles, window, cx);
            if let Some(value) = selected {
                select.set_selected_value(&value, window, cx);
            }
        });
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
        self.clip_model.update(cx, |select, cx| {
            select.set_selected_value(&draft.clip_model, window, cx);
            // A model no longer offered shows as the default; saving keeps
            // the form's choice.
            if select.selected_value().is_none() {
                select.set_selected_value(&None, window, cx);
            }
        });
        self.caption_style.update(cx, |select, cx| {
            select.set_selected_value(&draft.caption_style, window, cx)
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
            clip_model: self.clip_model.read(cx).selected_value().cloned().flatten(),
            caption_style: self
                .caption_style
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

    fn render_form(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
        let explained = |id: &'static str, label: Text, hint: Text, control: AnyElement| {
            kit::field(
                tr(bardo, label),
                Some(kit::info(id, None, tr(bardo, hint))),
                control,
                None,
            )
        };
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
                .child(
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
                        ),
                )
                .child(explained(
                    "channel-persona-info",
                    Text::ChannelPersona,
                    Text::ChannelPersonaHint,
                    Select::new(&self.persona)
                        .search_placeholder(tr(bardo, Text::PersonasTitle))
                        .into_any_element(),
                ))
                .child(explained(
                    "channel-clip-model-info",
                    Text::ChannelClipModel,
                    Text::ChannelClipModelHint,
                    Select::new(&self.clip_model).into_any_element(),
                ))
                .child(explained(
                    "channel-caption-style-info",
                    Text::ChannelCaptionStyle,
                    Text::ChannelCaptionStyleHint,
                    Select::new(&self.caption_style).into_any_element(),
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
        layout::screen(parts, cx)
    }
}
