//! Personas screen: the library on the left, the create/edit form on the
//! right with the ElevenLabs voice picker and the generation presets.
//! Personas are exported to and imported from package files through the
//! system's file dialogs; an imported persona whose voice is not (yet)
//! seen in the user's account shows a flag and how to fix it.
//! Rules, storage and the voice listing live in `bardo_app`; this file maps
//! the form to a `PersonaDraft` and results back to the screen.

use bardo_app::bardo_domain::{
    Channel, ChannelId, GenerationPresets, Persona, PersonaDraft, PersonaFieldError, PersonaId,
    Voice, VoiceFlag, VoiceRef,
};
use bardo_app::{Bardo, PersonaError, Text, VoiceStatus, persona_package_folder};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Entity, PathPromptOptions, SharedString, Subscription, Task,
    Window, div, px,
};

use crate::kit::{self, Tone};
use crate::shell::tr;

/// Field errors each control shows, and clears once the user edits it.
const NAME_ERRORS: &[PersonaFieldError] = &[
    PersonaFieldError::NameRequired,
    PersonaFieldError::NameTooLong,
];
const VOICE_ERRORS: &[PersonaFieldError] = &[PersonaFieldError::VoiceRequired];
const TONE_ERRORS: &[PersonaFieldError] = &[PersonaFieldError::ToneTooLong];
const SCRIPT_STYLE_ERRORS: &[PersonaFieldError] = &[PersonaFieldError::ScriptStyleTooLong];

/// One generation preset: its slider, labels and the error it can show.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Preset {
    Stability,
    Similarity,
    Style,
    Speed,
}

impl Preset {
    const ALL: [Preset; 4] = [
        Preset::Stability,
        Preset::Similarity,
        Preset::Style,
        Preset::Speed,
    ];

    fn label(self) -> Text {
        match self {
            Preset::Stability => Text::PresetStability,
            Preset::Similarity => Text::PresetSimilarity,
            Preset::Style => Text::PresetStyle,
            Preset::Speed => Text::PresetSpeed,
        }
    }

    fn hint(self) -> Text {
        match self {
            Preset::Stability => Text::PresetStabilityHint,
            Preset::Similarity => Text::PresetSimilarityHint,
            Preset::Style => Text::PresetStyleHint,
            Preset::Speed => Text::PresetSpeedHint,
        }
    }

    fn error(self) -> PersonaFieldError {
        match self {
            Preset::Stability => PersonaFieldError::StabilityOutOfRange,
            Preset::Similarity => PersonaFieldError::SimilarityOutOfRange,
            Preset::Style => PersonaFieldError::StyleOutOfRange,
            Preset::Speed => PersonaFieldError::SpeedOutOfRange,
        }
    }

    fn range(self) -> std::ops::RangeInclusive<u8> {
        match self {
            Preset::Speed => GenerationPresets::SPEED,
            _ => GenerationPresets::PERCENT,
        }
    }

    fn get(self, presets: &GenerationPresets) -> u8 {
        match self {
            Preset::Stability => presets.stability,
            Preset::Similarity => presets.similarity,
            Preset::Style => presets.style,
            Preset::Speed => presets.speed,
        }
    }

    fn set(self, presets: &mut GenerationPresets, value: u8) {
        match self {
            Preset::Stability => presets.stability = value,
            Preset::Similarity => presets.similarity = value,
            Preset::Style => presets.style = value,
            Preset::Speed => presets.speed = value,
        }
    }
}

/// What the form footer says after the last action.
enum Notice {
    Saved,
    Duplicated(String),
    Exported(String),
    Imported(String),
    Error(Text),
}

pub struct PersonasScreen {
    bardo: Entity<Bardo>,
    personas: Vec<Persona>,
    load_failed: bool,
    /// The persona being edited; `None` while creating a new one.
    editing: Option<PersonaId>,
    /// Channels that use the edited persona as their default.
    usage: Vec<Channel>,
    name: Entity<InputState>,
    tone: Entity<TextareaState>,
    script_style: Entity<TextareaState>,
    voice: Option<VoiceRef>,
    sliders: Vec<(Preset, Entity<SliderState>)>,
    picker_open: bool,
    /// The voice listing in flight; dropping it cancels the wait.
    listing: Option<Task<()>>,
    /// An export or import dialog that is open.
    dialog: Option<Task<()>>,
    picker_error: Option<Text>,
    /// Channels to confirm before the pending save goes through.
    confirm: Option<Vec<Channel>>,
    field_errors: Vec<PersonaFieldError>,
    notice: Option<Notice>,
    _subscriptions: Vec<Subscription>,
}

impl PersonasScreen {
    pub fn new(bardo: Entity<Bardo>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx));
        let tone = cx.new(|cx| TextareaState::new(window, cx).auto_grow(2, 8));
        let script_style = cx.new(|cx| TextareaState::new(window, cx).auto_grow(3, 10));
        let defaults = GenerationPresets::default();
        let sliders: Vec<_> = Preset::ALL
            .into_iter()
            .map(|preset| {
                let range = preset.range();
                let slider = cx.new(|_| {
                    SliderState::new()
                        .min(f32::from(*range.start()))
                        .max(f32::from(*range.end()))
                        .step(1.)
                        .default_value(f32::from(preset.get(&defaults)))
                });
                (preset, slider)
            })
            .collect();

        let mut subscriptions = vec![
            cx.subscribe(&name, |this, _, event, cx| {
                this.edited(NAME_ERRORS, event, cx)
            }),
            cx.subscribe(&tone, |this, _, event, cx| {
                this.edited(TONE_ERRORS, event, cx)
            }),
            cx.subscribe(&script_style, |this, _, event, cx| {
                this.edited(SCRIPT_STYLE_ERRORS, event, cx)
            }),
            cx.observe_in(&bardo, window, |this, _, window, cx| {
                this.relabel(window, cx)
            }),
        ];
        for (preset, slider) in &sliders {
            let preset = *preset;
            subscriptions.push(cx.subscribe(slider, move |this, _, event, cx| {
                if let SliderEvent::Change(_) = event {
                    this.field_errors.retain(|error| *error != preset.error());
                    this.touched(cx);
                }
            }));
        }

        let mut screen = Self {
            bardo,
            personas: Vec::new(),
            load_failed: false,
            editing: None,
            usage: Vec::new(),
            name,
            tone,
            script_style,
            voice: None,
            sliders,
            picker_open: false,
            listing: None,
            dialog: None,
            picker_error: None,
            confirm: None,
            field_errors: Vec::new(),
            notice: None,
            _subscriptions: subscriptions,
        };
        screen.reload_list(cx);
        screen.relabel(window, cx);
        // Open on the first persona, so the screen shows a filled form.
        if let Some(first) = screen.personas.first().map(|p| p.id) {
            screen.edit(first, window, cx);
        }
        screen
    }

    /// Re-reads personas and the edited persona's channels; channels may
    /// have changed their default persona on the channels screen.
    pub fn reload(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.reload_list(cx);
        self.reload_usage(cx);
        cx.notify();
    }

    fn reload_list(&mut self, cx: &mut Context<Self>) {
        match self.bardo.read(cx).personas() {
            Ok(personas) => {
                self.personas = personas;
                self.load_failed = false;
            }
            Err(_) => self.load_failed = true,
        }
    }

    fn reload_usage(&mut self, cx: &mut Context<Self>) {
        self.usage = match self.editing {
            Some(id) => self.bardo.read(cx).persona_usage(id).unwrap_or_default(),
            None => Vec::new(),
        };
    }

    /// Any edit hides the "saved" notice, so it never describes unsaved
    /// edits, and drops a pending confirmation made for other values.
    fn touched(&mut self, cx: &mut Context<Self>) {
        if matches!(
            self.notice,
            Some(Notice::Saved | Notice::Duplicated(_) | Notice::Imported(_))
        ) {
            self.notice = None;
        }
        self.confirm = None;
        cx.notify();
    }

    fn edited(&mut self, fields: &[PersonaFieldError], event: &InputEvent, cx: &mut Context<Self>) {
        if !matches!(event, InputEvent::Change) {
            return;
        }
        self.field_errors.retain(|error| !fields.contains(error));
        self.touched(cx);
    }

    /// Placeholders follow the interface language.
    fn relabel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let bardo = self.bardo.read(cx);
        let name = tr(bardo, Text::PersonaNamePlaceholder);
        let tone = tr(bardo, Text::PersonaTonePlaceholder);
        let style = tr(bardo, Text::PersonaScriptStylePlaceholder);
        self.name
            .update(cx, |input, cx| input.set_placeholder(name, window, cx));
        self.tone
            .update(cx, |input, cx| input.set_placeholder(tone, window, cx));
        self.script_style
            .update(cx, |input, cx| input.set_placeholder(style, window, cx));
        cx.notify();
    }

    fn fill(&mut self, draft: &PersonaDraft, window: &mut Window, cx: &mut Context<Self>) {
        self.name.update(cx, |input, cx| {
            input.set_value(draft.name.clone(), window, cx)
        });
        self.tone.update(cx, |input, cx| {
            input.set_value(draft.tone.clone(), window, cx)
        });
        self.script_style.update(cx, |input, cx| {
            input.set_value(draft.script_style.clone(), window, cx)
        });
        for (preset, slider) in &self.sliders {
            let value = f32::from(preset.get(&draft.presets));
            slider.update(cx, |slider, cx| slider.set_value(value, window, cx));
        }
        self.voice = draft.voice.clone();
    }

    fn presets(&self, cx: &App) -> GenerationPresets {
        let mut presets = GenerationPresets::default();
        for (preset, slider) in &self.sliders {
            let value = slider.read(cx).value().start().round().clamp(0., 255.) as u8;
            preset.set(&mut presets, value);
        }
        presets
    }

    fn draft(&self, cx: &App) -> PersonaDraft {
        PersonaDraft {
            name: self.name.read(cx).value().to_string(),
            voice: self.voice.clone(),
            tone: self.tone.read(cx).value().to_string(),
            script_style: self.script_style.read(cx).value().to_string(),
            presets: self.presets(cx),
        }
    }

    fn reset_feedback(&mut self) {
        self.field_errors.clear();
        self.notice = None;
        self.confirm = None;
    }

    fn start_new(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = None;
        self.usage.clear();
        self.reset_feedback();
        self.fill(&PersonaDraft::default(), window, cx);
        cx.notify();
    }

    fn edit(&mut self, id: PersonaId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(persona) = self.personas.iter().find(|p| p.id == id) else {
            return;
        };
        let draft = PersonaDraft::from(&persona.details);
        self.editing = Some(id);
        self.reset_feedback();
        self.fill(&draft, window, cx);
        self.reload_usage(cx);
        cx.notify();
    }

    /// Saves the form. `confirmed` names the channels the user agreed to
    /// change; the first save of a used persona confirms none and gets the
    /// list back to show.
    fn save(&mut self, confirmed: &[ChannelId], window: &mut Window, cx: &mut Context<Self>) {
        let draft = self.draft(cx);
        let bardo = self.bardo.read(cx);
        let result = match self.editing {
            Some(id) => bardo.update_persona(id, draft, confirmed),
            None => bardo.create_persona(draft),
        };
        match result {
            Ok(saved) => {
                self.reload_list(cx);
                self.editing = Some(saved.id);
                self.reset_feedback();
                // Show the normalized values (trimmed text).
                self.fill(&PersonaDraft::from(&saved.details), window, cx);
                self.reload_usage(cx);
                self.notice = Some(Notice::Saved);
            }
            Err(PersonaError::UsedByChannels(channels)) => {
                self.field_errors.clear();
                self.notice = None;
                self.confirm = Some(channels);
            }
            Err(error) => self.show_error(&error),
        }
        cx.notify();
    }

    fn confirm_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let confirmed: Vec<ChannelId> = self
            .confirm
            .iter()
            .flatten()
            .map(|channel| channel.id)
            .collect();
        self.save(&confirmed, window, cx);
    }

    fn duplicate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.editing else {
            return;
        };
        match self.bardo.read(cx).duplicate_persona(id) {
            Ok(copy) => {
                self.reload_list(cx);
                self.edit(copy.id, window, cx);
                self.notice = Some(Notice::Duplicated(copy.details.name().to_owned()));
            }
            Err(error) => self.show_error(&error),
        }
        cx.notify();
    }

    /// Asks where to save the edited persona's package, then writes it.
    fn export(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.editing else {
            return;
        };
        let name = match self.bardo.read(cx).persona_package_name(id) {
            Ok(name) => name,
            Err(error) => {
                self.show_error(&error);
                cx.notify();
                return;
            }
        };
        let chosen = cx.prompt_for_new_path(&persona_package_folder(), Some(&name));
        self.dialog = Some(cx.spawn(async move |this, cx| {
            let chosen = chosen.await;
            let _ = this.update(cx, |this, cx| {
                this.dialog = None;
                match chosen {
                    Ok(Ok(Some(path))) => match this.bardo.read(cx).export_persona_to(id, &path) {
                        Ok(()) => {
                            this.notice = Some(Notice::Exported(path.display().to_string()));
                        }
                        Err(error) => this.show_error(&error),
                    },
                    // Cancelled.
                    Ok(Ok(None)) | Err(_) => {}
                    Ok(Err(_)) => this.notice = Some(Notice::Error(Text::FileDialogFailed)),
                }
                cx.notify();
            });
        }));
    }

    /// Asks for a package file, imports it and opens the new persona. Its
    /// voice is checked right away when no listing has been made yet.
    fn import(&mut self, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(tr(self.bardo.read(cx), Text::ImportPersonaDialog)),
        });
        self.dialog = Some(cx.spawn(async move |this, cx| {
            let chosen = chosen.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.dialog = None;
                match chosen {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.first() {
                            this.import_from(path, window, cx);
                        }
                    }
                    Ok(Ok(None)) | Err(_) => {}
                    Ok(Err(_)) => this.notice = Some(Notice::Error(Text::FileDialogFailed)),
                }
                cx.notify();
            });
        }));
    }

    fn import_from(&mut self, path: &std::path::Path, window: &mut Window, cx: &mut Context<Self>) {
        match self.bardo.read(cx).import_persona_from(path) {
            Ok(imported) => {
                self.reload_list(cx);
                self.edit(imported.id, window, cx);
                self.notice = Some(Notice::Imported(imported.details.name().to_owned()));
                if imported.voice_flag == Some(VoiceFlag::Unchecked) && self.listing.is_none() {
                    self.load_voices(cx);
                }
            }
            Err(error) => self.show_error(&error),
        }
    }

    /// The stored flag of the persona being edited.
    fn edited_flag(&self) -> Option<VoiceFlag> {
        let id = self.editing?;
        self.personas.iter().find(|p| p.id == id)?.voice_flag
    }

    fn show_error(&mut self, error: &PersonaError) {
        self.field_errors = error.field_errors().to_vec();
        self.notice = error.form_message().map(Notice::Error);
        self.confirm = None;
    }

    fn toggle_picker(&mut self, cx: &mut Context<Self>) {
        self.picker_open = !self.picker_open;
        if self.picker_open && self.bardo.read(cx).voice_list().is_none() {
            self.load_voices(cx);
        }
        cx.notify();
    }

    /// Lists the account's voices on a background thread; the screen stays
    /// responsive and shows them when they arrive.
    fn load_voices(&mut self, cx: &mut Context<Self>) {
        let listing = match self.bardo.read(cx).voice_listing() {
            Ok(listing) => listing,
            Err(error) => {
                self.picker_error = error.form_message();
                cx.notify();
                return;
            }
        };
        let bardo = self.bardo.clone();
        let task = cx.spawn(async move |this, cx| {
            let list = cx
                .background_executor()
                .spawn(async move { listing.run() })
                .await;
            bardo.update(cx, |bardo, cx| {
                bardo.record_voice_list(list);
                cx.notify();
            });
            let _ = this.update(cx, |this, cx| {
                this.listing = None;
                // The listing may have cleared or set voice flags.
                this.reload_list(cx);
                cx.notify();
            });
        });
        self.picker_error = None;
        self.listing = Some(task);
        cx.notify();
    }

    fn pick(&mut self, voice: VoiceRef, cx: &mut Context<Self>) {
        self.voice = Some(voice);
        self.field_errors
            .retain(|error| !VOICE_ERRORS.contains(error));
        self.touched(cx);
    }

    fn render_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();

        let rows: Vec<AnyElement> = self
            .personas
            .iter()
            .enumerate()
            .map(|(ix, persona)| {
                let id = persona.id;
                let selected = self.editing == Some(id);
                kit::list_row(("persona", ix), selected, cx)
                    .child(div().child(SharedString::from(persona.details.name().to_owned())))
                    .child(div().text_xs().text_color(theme.muted_foreground).child(
                        SharedString::from(persona.details.voice().name().to_owned()),
                    ))
                    .children(persona.voice_flag.map(|flag| {
                        h_flex().child(kit::status(
                            Tone::Warning,
                            tr(bardo, Text::VoiceFlagTag(flag)),
                            cx,
                        ))
                    }))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.edit(id, window, cx)
                    }))
                    .into_any_element()
            })
            .collect();

        let body: AnyElement = if self.load_failed {
            div()
                .p_3()
                .child(kit::notice(
                    Tone::Danger,
                    tr(bardo, Text::PersonasNotLoaded),
                    cx,
                ))
                .into_any_element()
        } else if rows.is_empty() {
            div()
                .p_3()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(tr(bardo, Text::PersonasEmpty))
                .into_any_element()
        } else {
            v_flex()
                .id("persona-list")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p_2()
                .gap_1()
                .children(rows)
                .into_any_element()
        };

        kit::side_panel(cx)
            .w(px(280.))
            .h_full()
            .child(
                h_flex()
                    .p_3()
                    .justify_between()
                    .child(
                        h_flex()
                            .gap_1()
                            .child(div().font_semibold().child(tr(bardo, Text::PersonasTitle)))
                            .child(kit::info(
                                "personas-info",
                                None,
                                tr(bardo, Text::PersonasHint),
                            )),
                    )
                    .child(
                        Button::new("new-persona")
                            .small()
                            .label(tr(bardo, Text::NewPersona))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.start_new(window, cx)
                            })),
                    ),
            )
            .child(
                h_flex().px_3().pb_2().child(
                    Button::new("import-persona")
                        .small()
                        .outline()
                        .label(tr(bardo, Text::ImportPersona))
                        .disabled(self.dialog.is_some())
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.import(cx))),
                ),
            )
            .child(body)
    }

    fn render_voice(&self, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let loading = self.listing.is_some();

        let current: AnyElement =
            match &self.voice {
                Some(voice) => {
                    let status = match bardo.voice_status(voice) {
                        VoiceStatus::Available => Some((Text::VoiceAvailable, Tone::Success)),
                        VoiceStatus::Missing => Some((Text::VoiceMissing, Tone::Warning)),
                        VoiceStatus::Unknown => None,
                    };
                    v_flex()
                        .gap_0p5()
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    div()
                                        .font_medium()
                                        .child(SharedString::from(voice.name().to_owned())),
                                )
                                .child(div().text_xs().text_color(theme.muted_foreground).child(
                                    SharedString::from(format!(
                                        "{} · {}",
                                        bardo.text(Text::ProviderName(voice.provider())),
                                        voice.id()
                                    )),
                                )),
                        )
                        .children(status.map(|(text, tone)| {
                            h_flex().child(kit::status(tone, tr(bardo, text), cx))
                        }))
                        .into_any_element()
                }
                None => div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(tr(bardo, Text::PersonaVoiceNone))
                    .into_any_element(),
            };

        let toggle = if self.picker_open {
            Text::HideVoices
        } else {
            Text::ChooseVoice
        };
        let buttons = h_flex()
            .gap_2()
            .child(
                Button::new("toggle-voices")
                    .small()
                    .outline()
                    .label(tr(bardo, toggle))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_picker(cx))),
            )
            .when(self.picker_open, |row| {
                row.child(
                    Button::new("reload-voices")
                        .small()
                        .ghost()
                        .label(tr(bardo, Text::ReloadVoices))
                        .loading(loading)
                        .disabled(loading)
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.load_voices(cx))),
                )
            });

        let picker = self
            .picker_open
            .then(|| self.render_picker(cx).into_any_element());
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let error = self
            .field_errors
            .iter()
            .find(|e| VOICE_ERRORS.contains(e))
            .map(|error| {
                div()
                    .text_xs()
                    .text_color(theme.danger)
                    .child(tr(bardo, Text::PersonaFieldError(*error)))
            });

        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .font_medium()
                            .child(tr(bardo, Text::PersonaVoice)),
                    )
                    .child(kit::info(
                        "persona-voice-info",
                        None,
                        tr(bardo, Text::PersonaVoiceHint),
                    )),
            )
            .child(current)
            .child(buttons)
            .children(picker)
            .children(error)
            .into_any_element()
    }

    fn render_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let message = |text: SharedString, color| {
            div()
                .p_3()
                .text_sm()
                .text_color(color)
                .child(text)
                .into_any_element()
        };

        if let Some(error) = self.picker_error {
            return message(tr(bardo, error), theme.danger);
        }
        if self.listing.is_some() {
            return message(tr(bardo, Text::LoadingVoices), theme.muted_foreground);
        }
        let voices: &[Voice] = match bardo.voice_list().map(|list| &list.voices) {
            None => return message(tr(bardo, Text::LoadingVoices), theme.muted_foreground),
            Some(Err(failure)) => {
                return v_flex()
                    .p_3()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.danger)
                            .child(tr(bardo, Text::VoicesFailed)),
                    )
                    .child(div().text_xs().text_color(theme.muted_foreground).child(
                        SharedString::from(
                            bardo.text_with(Text::KeyCheckDetail, &[("detail", &failure.detail)]),
                        ),
                    ))
                    .into_any_element();
            }
            Some(Ok(voices)) if voices.is_empty() => {
                return message(tr(bardo, Text::VoicesEmpty), theme.muted_foreground);
            }
            Some(Ok(voices)) => voices,
        };

        let rows: Vec<AnyElement> = voices
            .iter()
            .enumerate()
            .map(|(ix, voice)| {
                let selected = self
                    .voice
                    .as_ref()
                    .is_some_and(|current| current.same_voice(&voice.reference));
                let mut traits = vec![
                    bardo
                        .text(Text::VoiceCategoryName(voice.category))
                        .into_owned(),
                ];
                traits.extend(voice.labels.iter().cloned());
                let reference = voice.reference.clone();
                kit::list_row(("voice", ix), selected, cx)
                    .child(
                        h_flex()
                            .justify_between()
                            .gap_2()
                            .child(
                                div()
                                    .font_medium()
                                    .child(SharedString::from(voice.reference.name().to_owned())),
                            )
                            .when(selected, |row| {
                                row.child(kit::status_with(
                                    Tone::Accent,
                                    IconName::Check,
                                    tr(bardo, Text::VoiceSelected),
                                    cx,
                                ))
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(SharedString::from(traits.join(" · "))),
                    )
                    .when(!voice.description.is_empty(), |row| {
                        row.child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(SharedString::from(voice.description.clone())),
                        )
                    })
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.pick(reference.clone(), cx)
                    }))
                    .into_any_element()
            })
            .collect();

        kit::well(cx)
            .p_0()
            .child(
                div()
                    .px_3()
                    .pt_2()
                    .text_xs()
                    .font_medium()
                    .text_color(theme.muted_foreground)
                    .child(tr(bardo, Text::VoicesTitle)),
            )
            .child(
                v_flex()
                    .id("voice-list")
                    .max_h(px(280.))
                    .overflow_y_scroll()
                    .p_1()
                    .gap_0p5()
                    .children(rows),
            )
            .into_any_element()
    }

    fn render_presets(&self, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let rows: Vec<AnyElement> = self
            .sliders
            .iter()
            .map(|(preset, slider)| {
                let value = slider.read(cx).value().start().round();
                let error = self.field_errors.contains(&preset.error()).then(|| {
                    div()
                        .text_xs()
                        .text_color(theme.danger)
                        .child(tr(bardo, Text::PersonaFieldError(preset.error())))
                });
                v_flex()
                    .gap_1()
                    .child(
                        h_flex()
                            .justify_between()
                            .child(
                                h_flex()
                                    .gap_1()
                                    .child(div().text_sm().child(tr(bardo, preset.label())))
                                    .child(kit::info(
                                        ("preset-info", *preset as usize),
                                        None,
                                        tr(bardo, preset.hint()),
                                    )),
                            )
                            .child(div().text_sm().font_medium().child(SharedString::from(
                                bardo.text_with(Text::PresetPercent, &[("n", &value.to_string())]),
                            ))),
                    )
                    .child(Slider::new(slider).horizontal())
                    .children(error)
                    .into_any_element()
            })
            .collect();

        v_flex()
            .gap_3()
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .font_medium()
                            .child(tr(bardo, Text::PersonaPresets)),
                    )
                    .child(kit::info(
                        "persona-presets-info",
                        None,
                        tr(bardo, Text::PersonaPresetsHint),
                    )),
            )
            .child(
                div()
                    .grid()
                    .grid_cols(2)
                    .items_start()
                    .gap_x_6()
                    .gap_y_4()
                    .children(rows),
            )
            .into_any_element()
    }

    fn render_confirm(&self, channels: &[Channel], cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        kit::card(cx)
            .p_4()
            .gap_2()
            .border_color(theme.warning)
            .child(
                div()
                    .font_semibold()
                    .child(SharedString::from(bardo.text_with(
                        Text::PersonaConfirmTitle,
                        &[("n", &channels.len().to_string())],
                    ))),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(tr(bardo, Text::PersonaConfirmHint)),
            )
            .children(channels.iter().map(|channel| {
                div()
                    .text_sm()
                    .child(SharedString::from(format!("• {}", channel.details.name())))
            }))
            .child(
                h_flex()
                    .gap_2()
                    .pt_1()
                    .child(
                        Button::new("confirm-persona-save")
                            .warning()
                            .label(tr(bardo, Text::ConfirmPersonaSave))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.confirm_save(window, cx)
                            })),
                    )
                    .child(
                        Button::new("cancel-persona-save")
                            .ghost()
                            .label(tr(bardo, Text::CancelPersonaSave))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.confirm = None;
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    /// Why the edited persona cannot narrate, and how to fix it.
    fn render_flag(&self, flag: VoiceFlag, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let loading = self.listing.is_some();
        kit::card(cx)
            .p_3()
            .gap_2()
            .border_color(theme.warning)
            .child(h_flex().child(kit::status(
                Tone::Warning,
                tr(bardo, Text::VoiceFlagTag(flag)),
                cx,
            )))
            .child(
                div()
                    .text_sm()
                    .child(tr(bardo, Text::VoiceFlagExplanation(flag))),
            )
            .children(
                self.picker_error
                    .or_else(|| {
                        // The last check reached nothing to compare with.
                        matches!(bardo.voice_list().map(|list| &list.voices), Some(Err(_)))
                            .then_some(Text::VoicesFailed)
                    })
                    .filter(|_| !loading)
                    .map(|error| {
                        div()
                            .text_xs()
                            .text_color(theme.danger)
                            .child(tr(bardo, error))
                    }),
            )
            .child(
                h_flex().child(
                    Button::new("check-voices")
                        .small()
                        .outline()
                        .label(tr(bardo, Text::CheckVoices))
                        .loading(loading)
                        .disabled(loading)
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.load_voices(cx))),
                ),
            )
            .into_any_element()
    }

    fn render_form(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let flag = self.edited_flag().map(|flag| self.render_flag(flag, cx));
        let voice = self.render_voice(cx);
        let presets = self.render_presets(cx);
        let confirm = self
            .confirm
            .as_deref()
            .map(|channels| self.render_confirm(channels, cx));
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let error_for = |fields: &[PersonaFieldError]| -> Option<AnyElement> {
            let error = self.field_errors.iter().find(|e| fields.contains(e))?;
            Some(
                div()
                    .text_xs()
                    .text_color(theme.danger)
                    .child(tr(bardo, Text::PersonaFieldError(*error)))
                    .into_any_element(),
            )
        };
        let field = |label: Text, control: AnyElement, below: Option<AnyElement>| {
            kit::field(tr(bardo, label), None, control, below)
        };

        let (title, action) = match self.editing {
            Some(_) => (Text::EditPersonaTitle, Text::SavePersona),
            None => (Text::NewPersonaTitle, Text::CreatePersona),
        };
        let usage = self.editing.map(|_| {
            let text: SharedString = if self.usage.is_empty() {
                tr(bardo, Text::PersonaUnused)
            } else {
                let names: Vec<_> = self.usage.iter().map(|c| c.details.name()).collect();
                bardo
                    .text_with(Text::PersonaUsedBy, &[("channels", &names.join(", "))])
                    .into()
            };
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(text)
        });
        // Shrinks and wraps beside the buttons; a copy's name can be long.
        let notice = self.notice.as_ref().map(|notice| {
            let (tone, text): (Tone, SharedString) = match notice {
                Notice::Saved => (Tone::Success, tr(bardo, Text::PersonaSaved)),
                Notice::Duplicated(name) => (
                    Tone::Success,
                    bardo
                        .text_with(Text::PersonaDuplicated, &[("name", name)])
                        .into(),
                ),
                Notice::Exported(path) => (
                    Tone::Success,
                    bardo
                        .text_with(Text::PersonaExported, &[("path", path)])
                        .into(),
                ),
                Notice::Imported(name) => (
                    Tone::Success,
                    bardo
                        .text_with(Text::PersonaImported, &[("name", name)])
                        .into(),
                ),
                Notice::Error(text) => (Tone::Danger, tr(bardo, *text)),
            };
            kit::notice(tone, text, cx)
        });

        v_flex()
            .id("persona-form")
            .flex_1()
            .h_full()
            .overflow_y_scroll()
            .child(
                v_flex()
                    .max_w(px(720.))
                    .p_6()
                    .gap_4()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(kit::title(tr(bardo, title)))
                            .children(usage),
                    )
                    .children(flag)
                    .child(
                        kit::card(cx)
                            .p_4()
                            .gap_4()
                            .child(field(
                                Text::PersonaName,
                                Input::new(&self.name).into_any_element(),
                                error_for(NAME_ERRORS),
                            ))
                            .child(voice)
                            .child(field(
                                Text::PersonaTone,
                                Textarea::new(&self.tone).into_any_element(),
                                error_for(TONE_ERRORS),
                            ))
                            .child(field(
                                Text::PersonaScriptStyle,
                                Textarea::new(&self.script_style).into_any_element(),
                                error_for(SCRIPT_STYLE_ERRORS),
                            ))
                            .child(presets),
                    )
                    .children(confirm)
                    .child(
                        h_flex()
                            .gap_3()
                            .child(
                                Button::new("save-persona")
                                    .primary()
                                    .label(tr(bardo, action))
                                    .disabled(self.confirm.is_some())
                                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                        this.save(&[], window, cx)
                                    })),
                            )
                            .when(self.editing.is_some(), |row| {
                                row.child(
                                    Button::new("duplicate-persona")
                                        .outline()
                                        .label(tr(bardo, Text::DuplicatePersona))
                                        .on_click(cx.listener(
                                            |this, _: &ClickEvent, window, cx| {
                                                this.duplicate(window, cx)
                                            },
                                        )),
                                )
                                .child(
                                    Button::new("export-persona")
                                        .outline()
                                        .label(tr(bardo, Text::ExportPersona))
                                        .disabled(self.dialog.is_some())
                                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                            this.export(cx)
                                        })),
                                )
                                .child(kit::info(
                                    "persona-export-info",
                                    None,
                                    tr(bardo, Text::PersonaExportHint),
                                ))
                            })
                            .children(notice.map(|notice| notice.flex_1())),
                    ),
            )
    }
}

impl Render for PersonasScreen {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .size_full()
            .items_start()
            .child(self.render_list(cx))
            .child(self.render_form(cx))
    }
}
