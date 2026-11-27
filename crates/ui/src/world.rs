//! World screen (#337): the characters that come back across videos as a
//! collection, and the picked one's name, appearance, notes and reference
//! pictures as its inspector. The same screen shows the open project's
//! world on World and the global library's on its World tab. Drawing a
//! picture is paid and shows its estimate first; everything else is free.
//! Rules, storage and the drawing job live in `bardo_app`; this file maps
//! the form to a `CharacterDraft` and results back to the screen.

use std::rc::Rc;
use std::time::Duration;

use bardo_app::bardo_domain::{
    CharacterDetails, CharacterDraft, CharacterFieldError, CharacterId, Look, ProjectId,
    ReferencePictureId, StyleChoice,
};
use bardo_app::{
    Bardo, BudgetConsent, CharacterCard, Control, Destination, PictureView, SpendEstimate, Text,
    TourAnchor, WorldError,
};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::searchable_list::{SearchableListItem, SearchableVec};
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IndexPath, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Entity, ObjectFit, ScrollHandle, SharedString, Subscription, Task,
    Window, div, img, px,
};

use crate::kit::{self, Tone};
use crate::parts::{Collection, CollectionKind, Header, Inspector, ScreenParts, Tile};
use crate::shell::tr;
use crate::spend::{budget_question, estimate_note};
use crate::{guide, layout};

const NAME_ERRORS: &[CharacterFieldError] = &[
    CharacterFieldError::NameRequired,
    CharacterFieldError::NameTooLong,
];
const APPEARANCE_ERRORS: &[CharacterFieldError] = &[
    CharacterFieldError::AppearanceRequired,
    CharacterFieldError::AppearanceTooLong,
];
const NOTES_ERRORS: &[CharacterFieldError] = &[CharacterFieldError::NotesTooLong];

/// The side of a picture's thumbnail.
const THUMBNAIL: f32 = 132.;

/// How often a drawing in progress is checked.
const POLL_EVERY: Duration = Duration::from_millis(250);

/// Whose world the screen shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldScope {
    /// The open project's, on World.
    Project,
    /// The global library's, on its World tab.
    Global,
}

/// What the form footer says after the last action.
enum Notice {
    Saved,
    Duplicated(String),
    Archived,
    Restored,
    Deleted(String),
    Error(SharedString),
}

/// What the pictures section says after the last action.
#[derive(Clone)]
enum PictureNotice {
    Started,
    Kept,
    Discarded,
    Removed,
    Error(Text),
}

/// One look of the picker: the look and its name.
#[derive(Clone)]
struct LookChoice {
    value: Look,
    title: SharedString,
}

impl SearchableListItem for LookChoice {
    type Value = Look;

    fn title(&self) -> SharedString {
        self.title.clone()
    }

    fn value(&self) -> &Look {
        &self.value
    }
}

type LookSelect = Entity<SelectState<SearchableVec<LookChoice>>>;

pub struct WorldScreen {
    bardo: Entity<Bardo>,
    scope: WorldScope,
    /// The project whose world shows; `None` for the global library (and,
    /// on World, while no project is open).
    owner: Option<ProjectId>,
    /// World with no open project: nothing to show.
    no_project: bool,
    cards: Vec<CharacterCard>,
    load_failed: bool,
    show_archived: bool,
    form_scroll: ScrollHandle,
    /// The character being edited; `None` while creating one.
    editing: Option<CharacterId>,
    name: Entity<InputState>,
    appearance: Entity<TextareaState>,
    always: Entity<TextareaState>,
    never: Entity<TextareaState>,
    /// The saved fields the form was filled from; `None` for a new one.
    filled: Option<CharacterDetails>,
    field_errors: Vec<CharacterFieldError>,
    notice: Option<Notice>,
    confirm_delete: bool,
    /// A new appearance waits for the user to accept what it drops: the
    /// looks' names and the videos' count.
    confirm_drops: Option<(String, usize)>,
    /// The looks a picture can be drawn in, as the picker lists them.
    looks: Vec<(Look, SharedString)>,
    look: LookSelect,
    estimate: Option<SpendEstimate>,
    /// A drawing past the budget, waiting for the user's answer.
    ask: Option<SpendEstimate>,
    picture_notice: Option<PictureNotice>,
    confirm_remove: Option<ReferencePictureId>,
    revision: u64,
    _poll: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl WorldScreen {
    pub fn new(
        bardo: Entity<Bardo>,
        scope: WorldScope,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx));
        let appearance = cx.new(|cx| TextareaState::new(window, cx).auto_grow(3, 10));
        let always = cx.new(|cx| TextareaState::new(window, cx).auto_grow(2, 6));
        let never = cx.new(|cx| TextareaState::new(window, cx).auto_grow(2, 6));
        let look = cx.new(|cx| {
            SelectState::new(SearchableVec::new(Vec::new()), None, window, cx).searchable(true)
        });
        let subscriptions = vec![
            cx.subscribe(&name, |this, _, event, cx| {
                this.edited(NAME_ERRORS, event, cx)
            }),
            cx.subscribe(&appearance, |this, _, event, cx| {
                this.edited(APPEARANCE_ERRORS, event, cx)
            }),
            cx.subscribe(&always, |this, _, event, cx| {
                this.edited(NOTES_ERRORS, event, cx)
            }),
            cx.subscribe(&never, |this, _, event, cx| {
                this.edited(NOTES_ERRORS, event, cx)
            }),
            cx.subscribe(
                &look,
                |this, _, _: &SelectEvent<SearchableVec<LookChoice>>, cx| {
                    this.ask = None;
                    cx.notify();
                },
            ),
            cx.observe_in(&bardo, window, |this, _, window, cx| {
                this.relabel(window, cx)
            }),
        ];
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL_EVERY).await;
                if this.update(cx, |this, cx| this.poll(cx)).is_err() {
                    break;
                }
            }
        });
        let revision = bardo.read(cx).jobs_revision();
        let mut screen = Self {
            bardo,
            scope,
            owner: None,
            no_project: false,
            cards: Vec::new(),
            load_failed: false,
            show_archived: false,
            form_scroll: ScrollHandle::new(),
            editing: None,
            name,
            appearance,
            always,
            never,
            filled: None,
            field_errors: Vec::new(),
            notice: None,
            confirm_delete: false,
            confirm_drops: None,
            looks: Vec::new(),
            look,
            estimate: None,
            ask: None,
            picture_notice: None,
            confirm_remove: None,
            revision,
            _poll: poll,
            _subscriptions: subscriptions,
        };
        screen.resolve_owner(cx);
        screen.reload_list(cx);
        screen.relabel(window, cx);
        if let Some(first) = screen.first_listed() {
            screen.edit(first, window, cx);
        }
        screen
    }

    /// Re-reads the world: another project may be open, or a restored
    /// backup may have changed it.
    pub fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let owner = self.owner;
        self.resolve_owner(cx);
        self.reload_list(cx);
        self.relabel(window, cx);
        let stored = self.editing.and_then(|id| {
            self.cards
                .iter()
                .find(|card| card.character.id == id)
                .map(|card| card.character.details.clone())
        });
        match (self.editing, stored) {
            _ if owner != self.owner => match self.first_listed() {
                Some(first) => self.edit(first, window, cx),
                None => self.start_new(window, cx),
            },
            (Some(id), Some(stored))
                if self.filled.as_ref() != Some(&stored) && !self.dirty(cx) =>
            {
                self.edit(id, window, cx)
            }
            (Some(_), None) => self.start_new(window, cx),
            _ => {}
        }
        cx.notify();
    }

    /// Reads which project's world shows: only when the screen reloads,
    /// so a form is never saved into a project opened meanwhile.
    fn resolve_owner(&mut self, cx: &App) {
        self.owner = match self.scope {
            WorldScope::Global => None,
            WorldScope::Project => self
                .bardo
                .read(cx)
                .open_project()
                .ok()
                .flatten()
                .map(|p| p.id),
        };
        self.no_project = self.scope == WorldScope::Project && self.owner.is_none();
    }

    fn reload_list(&mut self, cx: &mut Context<Self>) {
        let bardo = self.bardo.read(cx);
        if self.no_project {
            self.cards.clear();
            self.load_failed = false;
            return;
        }
        match bardo.world(self.owner, true) {
            Ok(cards) => {
                self.cards = cards;
                self.load_failed = false;
            }
            Err(_) => self.load_failed = true,
        }
        self.estimate = bardo.world_picture_estimate().ok();
    }

    /// Reloads when a picture job moved.
    fn poll(&mut self, cx: &mut Context<Self>) {
        let revision = self.bardo.read(cx).jobs_revision();
        if revision == self.revision {
            return;
        }
        self.revision = revision;
        let before: Vec<(CharacterId, bool, usize)> = self.signature();
        self.reload_list(cx);
        if self.signature() != before {
            cx.notify();
        }
    }

    fn signature(&self) -> Vec<(CharacterId, bool, usize)> {
        self.cards
            .iter()
            .map(|card| (card.character.id, card.drawing, card.pictures.len()))
            .collect()
    }

    fn card(&self, id: CharacterId) -> Option<&CharacterCard> {
        self.cards.iter().find(|card| card.character.id == id)
    }

    fn editing_card(&self) -> Option<&CharacterCard> {
        self.editing.and_then(|id| self.card(id))
    }

    /// The characters the list shows: archived ones only when asked for,
    /// and always the one being edited.
    fn listed(&self) -> impl Iterator<Item = &CharacterCard> {
        self.cards.iter().filter(|card| {
            self.show_archived
                || !card.character.is_archived()
                || self.editing == Some(card.character.id)
        })
    }

    fn first_listed(&self) -> Option<CharacterId> {
        self.listed().next().map(|card| card.character.id)
    }

    fn archived_count(&self) -> usize {
        self.cards
            .iter()
            .filter(|card| card.character.is_archived())
            .count()
    }

    /// Whether there are characters to show: the screen offers its tour
    /// only then.
    pub fn has_content(&self) -> bool {
        !self.load_failed && !self.cards.is_empty()
    }

    fn touched(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.notice, Some(Notice::Error(_)) | None) {
            self.notice = None;
        }
        self.confirm_delete = false;
        self.confirm_drops = None;
        cx.notify();
    }

    fn edited(
        &mut self,
        fields: &[CharacterFieldError],
        event: &InputEvent,
        cx: &mut Context<Self>,
    ) {
        if !matches!(event, InputEvent::Change) {
            return;
        }
        self.field_errors.retain(|error| !fields.contains(error));
        self.touched(cx);
    }

    /// The look's name: a catalog style, or a channel's notes.
    fn look_name(bardo: &Bardo, look: &Look, channel: &str) -> SharedString {
        match look {
            Look::Style(style) => tr(bardo, Text::VisualStyleName(StyleChoice::Catalog(*style))),
            Look::Notes { .. } if channel.is_empty() => {
                tr(bardo, Text::VisualStyleName(StyleChoice::FromNotes))
            }
            Look::Notes { .. } => bardo
                .text_with(Text::WorldPictureLookNotes, &[("channel", channel)])
                .into(),
        }
    }

    /// The name of a picture's look, as the picker names it; a channel's
    /// older notes read as notes.
    fn picture_look(&self, bardo: &Bardo, look: &Look) -> SharedString {
        self.looks
            .iter()
            .find(|(candidate, _)| candidate == look)
            .map(|(_, name)| name.clone())
            .unwrap_or_else(|| Self::look_name(bardo, look, ""))
    }

    /// Placeholders and the looks follow the interface language and the
    /// open project's channels.
    fn relabel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let bardo = self.bardo.read(cx);
        let looks: Vec<(Look, SharedString)> = bardo
            .world_looks()
            .unwrap_or_default()
            .into_iter()
            .map(|(look, channel)| {
                let name = Self::look_name(bardo, &look, &channel);
                (look, name)
            })
            .collect();
        let inputs = [(&self.name, Text::CharacterNamePlaceholder)]
            .map(|(input, text)| (input.clone(), tr(bardo, text)));
        let areas = [
            (&self.appearance, Text::CharacterAppearancePlaceholder),
            (&self.always, Text::CharacterAlwaysPlaceholder),
            (&self.never, Text::CharacterNeverPlaceholder),
        ]
        .map(|(area, text)| (area.clone(), tr(bardo, text)));
        for (input, text) in inputs {
            input.update(cx, |input, cx| input.set_placeholder(text, window, cx));
        }
        for (area, text) in areas {
            area.update(cx, |area, cx| area.set_placeholder(text, window, cx));
        }
        if looks != self.looks {
            let items = SearchableVec::new(
                looks
                    .iter()
                    .map(|(value, title)| LookChoice {
                        value: value.clone(),
                        title: title.clone(),
                    })
                    .collect::<Vec<_>>(),
            );
            self.look.update(cx, |select, cx| {
                let selected = select.selected_value().cloned();
                select.set_items(items, window, cx);
                match selected {
                    Some(value) if looks.iter().any(|(look, _)| *look == value) => {
                        select.set_selected_value(&value, window, cx)
                    }
                    _ if !looks.is_empty() => {
                        select.set_selected_index(Some(IndexPath::new(0)), window, cx)
                    }
                    _ => {}
                }
            });
            self.looks = looks;
        }
        cx.notify();
    }

    fn fill(&mut self, draft: &CharacterDraft, window: &mut Window, cx: &mut Context<Self>) {
        let name = draft.name.clone();
        self.name
            .update(cx, |input, cx| input.set_value(name, window, cx));
        for (area, value) in [
            (&self.appearance, &draft.appearance),
            (&self.always, &draft.always),
            (&self.never, &draft.never),
        ] {
            let value = value.clone();
            area.update(cx, |area, cx| area.set_value(value, window, cx));
        }
    }

    fn draft(&self, cx: &App) -> CharacterDraft {
        CharacterDraft {
            name: self.name.read(cx).value().to_string(),
            appearance: self.appearance.read(cx).value().to_string(),
            always: self.always.read(cx).value().to_string(),
            never: self.never.read(cx).value().to_string(),
        }
    }

    /// Whether the form differs from the saved fields it was filled from.
    fn dirty(&self, cx: &App) -> bool {
        match (&self.filled, CharacterDetails::validate(self.draft(cx))) {
            (Some(filled), Ok(now)) => *filled != now,
            _ => true,
        }
    }

    fn reset_feedback(&mut self) {
        self.field_errors.clear();
        self.notice = None;
        self.confirm_delete = false;
        self.confirm_drops = None;
        self.ask = None;
        self.picture_notice = None;
        self.confirm_remove = None;
    }

    fn start_new(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = None;
        self.filled = None;
        self.reset_feedback();
        self.fill(&CharacterDraft::default(), window, cx);
        cx.notify();
    }

    fn edit(&mut self, id: CharacterId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(details) = self.card(id).map(|card| card.character.details.clone()) else {
            return;
        };
        self.filled = Some(details.clone());
        self.editing = Some(id);
        self.reset_feedback();
        self.fill(&CharacterDraft::from(&details), window, cx);
        cx.notify();
    }

    fn show_error(&mut self, error: &WorldError, cx: &App) {
        self.confirm_delete = false;
        self.confirm_drops = None;
        match error {
            WorldError::Field(field) => self.field_errors = vec![*field],
            WorldError::DropsPictures { looks, videos } => {
                let bardo = self.bardo.read(cx);
                let names: Vec<String> = looks
                    .iter()
                    .map(|look| self.picture_look(bardo, look).to_string())
                    .collect();
                let names = match names.is_empty() {
                    true => bardo.text(Text::CharacterDropsNone).into_owned(),
                    false => names.join(", "),
                };
                self.confirm_drops = Some((names, videos.len()));
            }
            WorldError::InUse(videos) => {
                let text = self
                    .bardo
                    .read(cx)
                    .text_with(Text::CharacterInUse, &[("videos", &videos.join(", "))]);
                self.notice = Some(Notice::Error(text.into()));
            }
            other => self.notice = Some(Notice::Error(tr(self.bardo.read(cx), other.message()))),
        }
    }

    /// Saves the form; with `confirmed`, a new appearance drops the
    /// pictures. `false` when it could not be saved.
    fn save(&mut self, confirmed: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let draft = self.draft(cx);
        let bardo = self.bardo.read(cx);
        let result = match self.editing {
            Some(id) => bardo.edit_character(id, draft, confirmed),
            None if self.no_project => return false,
            None => bardo.create_character(self.owner, draft),
        };
        let saved = match result {
            Ok(saved) => {
                self.editing = Some(saved.id);
                self.reload_list(cx);
                self.field_errors.clear();
                self.confirm_delete = false;
                self.confirm_drops = None;
                self.fill(&CharacterDraft::from(&saved.details), window, cx);
                self.filled = Some(saved.details);
                self.notice = Some(Notice::Saved);
                true
            }
            Err(error) => {
                self.show_error(&error, cx);
                false
            }
        };
        cx.notify();
        saved
    }

    fn duplicate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.editing else {
            return;
        };
        if self.dirty(cx) && !self.save(false, window, cx) {
            return;
        }
        match self.bardo.read(cx).duplicate_character(id) {
            Ok(copy) => {
                self.reload_list(cx);
                self.edit(copy.id, window, cx);
                self.notice = Some(Notice::Duplicated(copy.name().to_owned()));
            }
            Err(error) => self.show_error(&error, cx),
        }
        cx.notify();
    }

    fn set_archived(&mut self, archived: bool, cx: &mut Context<Self>) {
        let Some(id) = self.editing else {
            return;
        };
        match self.bardo.read(cx).archive_character(id, archived) {
            Ok(()) => {
                self.reload_list(cx);
                self.confirm_delete = false;
                self.notice = Some(if archived {
                    Notice::Archived
                } else {
                    Notice::Restored
                });
            }
            Err(error) => self.show_error(&error, cx),
        }
        cx.notify();
    }

    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.editing else {
            return;
        };
        let name = self
            .card(id)
            .map(|card| card.character.name().to_owned())
            .unwrap_or_default();
        match self.bardo.read(cx).delete_character(id) {
            Ok(()) => {
                self.reload_list(cx);
                match self.first_listed() {
                    Some(next) => self.edit(next, window, cx),
                    None => self.start_new(window, cx),
                }
                self.notice = Some(Notice::Deleted(name));
            }
            Err(error) => self.show_error(&error, cx),
        }
        cx.notify();
    }

    fn picture_failed(&mut self, error: &WorldError) {
        self.picture_notice = Some(PictureNotice::Error(error.message()));
    }

    fn draw(&mut self, consent: BudgetConsent, cx: &mut Context<Self>) {
        let Some(id) = self.editing else {
            return;
        };
        let Some(look) = self.look.read(cx).selected_value().cloned() else {
            return;
        };
        self.ask = None;
        self.picture_notice = None;
        match self.bardo.read(cx).draw_world_picture(id, look, consent) {
            Ok(_) => {
                self.picture_notice = Some(PictureNotice::Started);
                self.reload_list(cx);
            }
            Err(WorldError::OverBudget(estimate)) => self.ask = Some(estimate),
            Err(error) => self.picture_failed(&error),
        }
        cx.notify();
    }

    fn keep(&mut self, picture: ReferencePictureId, cx: &mut Context<Self>) {
        let Some(id) = self.editing else {
            return;
        };
        match self.bardo.read(cx).keep_world_picture(id, picture) {
            Ok(()) => self.picture_notice = Some(PictureNotice::Kept),
            Err(error) => self.picture_failed(&error),
        }
        self.reload_list(cx);
        cx.notify();
    }

    fn discard(&mut self, picture: ReferencePictureId, kept: bool, cx: &mut Context<Self>) {
        let Some(id) = self.editing else {
            return;
        };
        self.confirm_remove = None;
        match self.bardo.read(cx).discard_world_picture(id, picture) {
            Ok(()) => {
                self.picture_notice = Some(if kept {
                    PictureNotice::Removed
                } else {
                    PictureNotice::Discarded
                })
            }
            Err(error) => self.picture_failed(&error),
        }
        self.reload_list(cx);
        cx.notify();
    }

    fn collection(&self, cx: &mut Context<Self>) -> Collection {
        let mut collection = Collection::new(CollectionKind::List, "character-list");
        let bardo = self.bardo.read(cx);
        let tiles: Vec<Tile> = self
            .listed()
            .enumerate()
            .map(|(ix, card)| {
                let id = card.character.id;
                let mut tile = Tile::new(
                    ("character", ix),
                    Rc::new(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.edit(id, window, cx)
                    })),
                );
                tile.selected = self.editing == Some(id);
                tile.title = Some(SharedString::from(card.character.name().to_owned()));
                tile.text = Some(SharedString::from(
                    card.character.details.appearance().to_owned(),
                ));
                tile.picture = card
                    .pictures
                    .iter()
                    .find(|view| view.picture.kept)
                    .map(|view| {
                        img(view.path.clone())
                            .size_full()
                            .object_fit(ObjectFit::Cover)
                            .into_any_element()
                    });
                tile.status = card.character.is_archived().then(|| {
                    kit::status(Tone::Neutral, tr(bardo, Text::CharacterArchivedTag), cx)
                        .into_any_element()
                });
                tile
            })
            .collect();
        let empty = if self.load_failed {
            kit::notice(Tone::Danger, tr(bardo, Text::WorldNotLoaded), cx).into_any_element()
        } else {
            let text = if self.no_project {
                Text::WorldNoProject
            } else {
                Text::WorldEmpty
            };
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(tr(bardo, text))
                .into_any_element()
        };
        collection.empty = Some(empty);
        if !self.load_failed {
            collection.tiles = tiles;
        }
        collection
    }

    fn error_for(&self, fields: &[CharacterFieldError], cx: &App) -> Option<AnyElement> {
        let error = self.field_errors.iter().find(|e| fields.contains(e))?;
        Some(
            div()
                .text_xs()
                .text_color(cx.theme().danger)
                .child(tr(self.bardo.read(cx), Text::CharacterFieldError(*error)))
                .into_any_element(),
        )
    }

    fn muted(text: impl IntoElement, cx: &App) -> AnyElement {
        div()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(text)
            .into_any_element()
    }

    fn render_picture(&self, ix: usize, view: &PictureView, cx: &mut Context<Self>) -> AnyElement {
        let id = view.picture.id;
        let kept = view.picture.kept;
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let look = self.picture_look(bardo, &view.picture.look);
        let origin = tr(
            bardo,
            Text::WorldPictureOrigin(view.picture.provenance.origin),
        );
        let actions: AnyElement = if !kept {
            h_flex()
                .flex_wrap()
                .gap_1()
                .child(
                    Button::new(("keep-world-picture", ix))
                        .xsmall()
                        .primary()
                        .label(tr(bardo, Text::WorldPictureKeep))
                        .on_click(
                            cx.listener(move |this, _: &ClickEvent, _, cx| this.keep(id, cx)),
                        ),
                )
                .child(
                    Button::new(("discard-world-picture", ix))
                        .xsmall()
                        .ghost()
                        .label(tr(bardo, Text::WorldPictureDiscard))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.discard(id, false, cx)
                        })),
                )
                .into_any_element()
        } else if self.confirm_remove == Some(id) {
            v_flex()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .child(tr(bardo, Text::WorldPictureRemoveAsk)),
                )
                .child(
                    h_flex()
                        .flex_wrap()
                        .gap_1()
                        .child(
                            Button::new(("confirm-remove-world-picture", ix))
                                .xsmall()
                                .danger()
                                .label(tr(bardo, Text::WorldPictureRemove))
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.discard(id, true, cx)
                                })),
                        )
                        .child(
                            Button::new(("cancel-remove-world-picture", ix))
                                .xsmall()
                                .ghost()
                                .label(tr(bardo, Text::CharacterCancel))
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.confirm_remove = None;
                                    cx.notify();
                                })),
                        ),
                )
                .into_any_element()
        } else {
            h_flex()
                .child(
                    Button::new(("remove-world-picture", ix))
                        .xsmall()
                        .outline()
                        .label(tr(bardo, Text::WorldPictureRemove))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.confirm_remove = Some(id);
                            cx.notify();
                        })),
                )
                .into_any_element()
        };
        v_flex()
            .w(px(THUMBNAIL + 24.))
            .gap_1()
            .child(
                div()
                    .flex_none()
                    .size(px(THUMBNAIL))
                    .rounded_md()
                    .overflow_hidden()
                    .border_1()
                    .border_color(if kept { theme.border } else { theme.warning })
                    .bg(theme.muted)
                    .child(
                        img(view.path.clone())
                            .size_full()
                            .object_fit(ObjectFit::Contain),
                    ),
            )
            .child(div().text_sm().font_medium().child(look))
            .when(!kept, |col| {
                col.child(h_flex().child(kit::status(
                    Tone::Warning,
                    tr(bardo, Text::WorldPictureWaiting),
                    cx,
                )))
            })
            .child(Self::muted(origin, cx))
            .child(actions)
            .into_any_element()
    }

    fn render_draw(&self, card: &CharacterCard, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let ask = self.ask.as_ref().map(|estimate| {
            budget_question(
                "world-picture-budget",
                bardo,
                estimate,
                cx,
                cx.listener(|this, _: &ClickEvent, _, cx| this.draw(BudgetConsent::Confirmed, cx)),
                cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.ask = None;
                    cx.notify();
                }),
            )
        });
        let drawing = card.drawing;
        // A picture is drawn from the saved appearance, so edits are saved
        // first.
        let dirty = self.dirty(cx);
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .items_end()
                    .child(div().min_w(px(200.)).flex_1().child(kit::field(
                        tr(bardo, Text::WorldPictureLook),
                        None,
                        Select::new(&self.look).into_any_element(),
                        None,
                    )))
                    .child(
                        Button::new("draw-world-picture")
                            .outline()
                            .label(tr(bardo, Text::WorldPictureDraw))
                            .loading(drawing)
                            .disabled(
                                drawing || dirty || self.ask.is_some() || self.looks.is_empty(),
                            )
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.draw(BudgetConsent::Ask, cx)
                            })),
                    ),
            )
            .when(drawing, |col| {
                col.child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(Spinner::new().small())
                        .child(Self::muted(tr(bardo, Text::WorldPictureDrawing), cx)),
                )
            })
            .children(
                self.estimate
                    .as_ref()
                    .and_then(|estimate| estimate_note(bardo, estimate, Text::EstimateCost, cx)),
            )
            .children(ask)
            .into_any_element()
    }

    fn render_picture_notice(&self, cx: &App) -> Option<AnyElement> {
        let bardo = self.bardo.read(cx);
        let (tone, text) = match self.picture_notice.as_ref()? {
            PictureNotice::Started => (Tone::Info, Text::WorldPictureStarted),
            PictureNotice::Kept => (Tone::Success, Text::WorldPictureKept),
            PictureNotice::Discarded => (Tone::Neutral, Text::WorldPictureDiscarded),
            PictureNotice::Removed => (Tone::Neutral, Text::WorldPictureRemoved),
            PictureNotice::Error(text) => (Tone::Danger, *text),
        };
        Some(kit::notice(tone, tr(bardo, text), cx).into_any_element())
    }

    fn render_pictures(&self, cx: &mut Context<Self>) -> AnyElement {
        let card = self.editing_card().cloned();
        let body: AnyElement = match &card {
            None => Self::muted(tr(self.bardo.read(cx), Text::WorldPicturesSaveFirst), cx),
            Some(card) => {
                let tiles: Vec<AnyElement> = card
                    .pictures
                    .iter()
                    .enumerate()
                    .map(|(ix, view)| self.render_picture(ix, view, cx))
                    .collect();
                let draw = self.render_draw(card, cx);
                let notice = self.render_picture_notice(cx);
                let bardo = self.bardo.read(cx);
                v_flex()
                    .gap_3()
                    .map(|col| {
                        if tiles.is_empty() {
                            col.child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(tr(bardo, Text::WorldPicturesNone)),
                            )
                        } else {
                            col.child(h_flex().flex_wrap().items_start().gap_4().children(tiles))
                        }
                    })
                    .child(draw)
                    .children(notice)
                    .into_any_element()
            }
        };
        let bardo = self.bardo.read(cx);
        kit::card(cx)
            .p_4()
            .gap_3()
            .child(
                v_flex().gap_1().child(
                    h_flex()
                        .gap_1()
                        .items_center()
                        .child(kit::section_heading(tr(bardo, Text::WorldPicturesTitle)))
                        .child(guide::info(
                            bardo,
                            "world-pictures-info",
                            tr(bardo, Text::WorldPicturesHint),
                            guide::refs::WORLD_PICTURES,
                        )),
                ),
            )
            .child(body)
            .into_any_element()
    }

    fn render_uses(&self, cx: &App) -> Option<AnyElement> {
        let card = self.editing_card()?;
        let bardo = self.bardo.read(cx);
        let text: SharedString = if card.uses.is_empty() {
            tr(bardo, Text::CharacterUsesNone)
        } else {
            card.uses
                .iter()
                .map(|used| match used.archived {
                    true => bardo.text_with(Text::CharacterUseArchived, &[("title", &used.title)]),
                    false => used.title.clone(),
                })
                .collect::<Vec<_>>()
                .join(", ")
                .into()
        };
        Some(
            v_flex()
                .gap_0p5()
                .child(
                    div()
                        .text_sm()
                        .font_medium()
                        .child(tr(bardo, Text::CharacterUses)),
                )
                .child(Self::muted(text, cx))
                .into_any_element(),
        )
    }

    fn render_delete(&self, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let name = self
            .editing_card()
            .map(|card| card.character.name().to_owned())
            .unwrap_or_default();
        kit::card(cx)
            .p_4()
            .gap_2()
            .border_color(theme.danger)
            .child(div().font_semibold().child(SharedString::from(
                bardo.text_with(Text::CharacterDeleteTitle, &[("name", &name)]),
            )))
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(tr(bardo, Text::CharacterDeleteHint)),
            )
            .child(
                h_flex()
                    .gap_2()
                    .pt_1()
                    .child(
                        Button::new("confirm-delete-character")
                            .danger()
                            .label(tr(bardo, Text::CharacterConfirmDelete))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.delete(window, cx)
                            })),
                    )
                    .child(
                        Button::new("cancel-delete-character")
                            .ghost()
                            .label(tr(bardo, Text::CharacterCancel))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.confirm_delete = false;
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    /// Asks before a new appearance drops the pictures and portraits.
    fn render_drops(&self, looks: &str, videos: usize, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        kit::card(cx)
            .p_4()
            .gap_2()
            .border_color(theme.warning)
            .child(
                div()
                    .font_semibold()
                    .child(tr(bardo, Text::CharacterDropsTitle)),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(SharedString::from(bardo.text_with(
                        Text::CharacterDropsPictures,
                        &[("looks", looks), ("videos", &videos.to_string())],
                    ))),
            )
            .child(
                h_flex()
                    .gap_2()
                    .pt_1()
                    .child(
                        Button::new("confirm-character-drops")
                            .warning()
                            .label(tr(bardo, Text::CharacterDropsConfirm))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.save(true, window, cx);
                            })),
                    )
                    .child(
                        Button::new("cancel-character-drops")
                            .ghost()
                            .label(tr(bardo, Text::CharacterCancel))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.confirm_drops = None;
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    fn render_form(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.no_project {
            return Self::muted(tr(self.bardo.read(cx), Text::WorldNoProject), cx);
        }
        let pictures = self.render_pictures(cx);
        let uses = self.render_uses(cx);
        let delete = self.confirm_delete.then(|| self.render_delete(cx));
        let drops = self
            .confirm_drops
            .clone()
            .map(|(looks, videos)| self.render_drops(&looks, videos, cx));
        let [name_error, appearance_error, notes_error] =
            [NAME_ERRORS, APPEARANCE_ERRORS, NOTES_ERRORS].map(|fields| self.error_for(fields, cx));
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let scroll = Some(&self.form_scroll);
        let archived = self
            .editing_card()
            .is_some_and(|card| card.character.is_archived());
        let (title, action) = match self.editing {
            Some(_) => (Text::CharacterEditTitle, Text::CharacterSave),
            None => (Text::CharacterNewTitle, Text::CharacterCreate),
        };
        let notice = self.notice.as_ref().map(|notice| {
            let (tone, text): (Tone, SharedString) = match notice {
                Notice::Saved => (Tone::Success, tr(bardo, Text::CharacterSaved)),
                Notice::Duplicated(name) => (
                    Tone::Success,
                    bardo
                        .text_with(Text::CharacterDuplicated, &[("name", name)])
                        .into(),
                ),
                Notice::Archived => (Tone::Neutral, tr(bardo, Text::CharacterArchived)),
                Notice::Restored => (Tone::Success, tr(bardo, Text::CharacterRestored)),
                Notice::Deleted(name) => (
                    Tone::Neutral,
                    bardo
                        .text_with(Text::CharacterDeleted, &[("name", name)])
                        .into(),
                ),
                Notice::Error(text) => (Tone::Danger, text.clone()),
            };
            kit::notice(tone, text, cx)
        });
        let actions = h_flex()
            .flex_wrap()
            .gap_3()
            .child(
                Button::new("save-character")
                    .primary()
                    .label(tr(bardo, action))
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.save(false, window, cx);
                    })),
            )
            .when(self.editing.is_some(), |row| {
                row.child(
                    Button::new("duplicate-character")
                        .outline()
                        .label(tr(bardo, Text::CharacterDuplicate))
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.duplicate(window, cx)
                        })),
                )
                .child(
                    Button::new("archive-character")
                        .outline()
                        .label(tr(
                            bardo,
                            if archived {
                                Text::CharacterRestore
                            } else {
                                Text::CharacterArchive
                            },
                        ))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.set_archived(!archived, cx)
                        })),
                )
                .child(
                    Button::new("delete-character")
                        .ghost()
                        .label(tr(bardo, Text::CharacterDelete))
                        .disabled(self.confirm_delete)
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.confirm_delete = true;
                            this.notice = None;
                            cx.notify();
                        })),
                )
                .child(guide::info(
                    bardo,
                    "character-actions-info",
                    tr(bardo, Text::CharacterActionsHint),
                    guide::refs::WORLD_ARCHIVE,
                ))
            })
            .children(notice.map(|notice| notice.flex_1().min_w_0()));
        let field = |label: Text, control: AnyElement, below: Option<AnyElement>| {
            kit::field(tr(bardo, label), None, control, below)
        };
        div()
            .child(
                v_flex()
                    .max_w(px(720.))
                    .gap_4()
                    .child(
                        v_flex().gap_1().child(kit::title(tr(bardo, title))).child(
                            div()
                                .text_sm()
                                .text_color(theme.muted_foreground)
                                .child(tr(bardo, Text::CharacterFormHint)),
                        ),
                    )
                    .child(
                        kit::card(cx).p_4().gap_4().child(kit::anchor_in(
                            TourAnchor::Control(Control::WorldCharacter),
                            v_flex()
                                .gap_4()
                                .child(field(
                                    Text::CharacterName,
                                    Input::new(&self.name).into_any_element(),
                                    name_error,
                                ))
                                .child(kit::field(
                                    tr(bardo, Text::CharacterAppearance),
                                    Some(guide::info(
                                        bardo,
                                        "character-appearance-info",
                                        tr(bardo, Text::CharacterAppearanceHint),
                                        guide::refs::WORLD_APPEARANCE,
                                    )),
                                    Textarea::new(&self.appearance).into_any_element(),
                                    appearance_error,
                                ))
                                .child(kit::field(
                                    tr(bardo, Text::CharacterAlways),
                                    Some(guide::info(
                                        bardo,
                                        "character-notes-info",
                                        tr(bardo, Text::CharacterNotesHint),
                                        guide::refs::WORLD_APPEARANCE,
                                    )),
                                    Textarea::new(&self.always).into_any_element(),
                                    None,
                                ))
                                .child(field(
                                    Text::CharacterNever,
                                    Textarea::new(&self.never).into_any_element(),
                                    notes_error,
                                ))
                                .children(uses),
                            scroll,
                        )),
                    )
                    .children(drops)
                    .children(delete)
                    .child(kit::anchor_in(
                        TourAnchor::Control(Control::WorldActions),
                        actions,
                        scroll,
                    ))
                    .child(kit::anchor_in(
                        TourAnchor::Control(Control::WorldPictures),
                        pictures,
                        scroll,
                    )),
            )
            .into_any_element()
    }
}

impl Render for WorldScreen {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let collection = self.collection(cx);
        let form = self.render_form(cx);
        let archived = self.archived_count();
        let bardo = self.bardo.read(cx);
        let mut header = Header::place(bardo, Destination::World, cx);
        let info = guide::info(
            bardo,
            "world-info",
            tr(bardo, Text::WorldHint),
            guide::refs::WORLD_CHARACTERS,
        )
        .into_any_element();
        header.info = guide::header_info(
            bardo,
            Destination::World,
            self.has_content(),
            Some(info),
            cx,
        );
        let mut actions = Vec::new();
        if archived > 0 {
            actions.push(
                Checkbox::new("show-archived-characters")
                    .small()
                    .checked(self.show_archived)
                    .label(SharedString::from(bardo.text_with(
                        Text::WorldShowArchived,
                        &[("n", &archived.to_string())],
                    )))
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        this.show_archived = *checked;
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }
        actions.push(
            Button::new("new-character")
                .small()
                .primary()
                .label(tr(bardo, Text::WorldNew))
                .disabled(self.no_project)
                .on_click(
                    cx.listener(|this, _: &ClickEvent, window, cx| this.start_new(window, cx)),
                )
                .into_any_element(),
        );
        header.actions = actions;
        let mut parts = ScreenParts::new(header);
        parts.collection = Some(collection);
        parts.inspector = Some(Inspector::new(vec![form]));
        parts.scroll = Some(self.form_scroll.clone());
        layout::screen(parts, cx)
    }
}
