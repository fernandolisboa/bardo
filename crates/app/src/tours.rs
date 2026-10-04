//! Guided tours (issue #105): the window dims, one component at a time
//! stays lit, and a card beside it says what it is and how to use it.
//!
//! A tour is a list of steps, each pointing at a [`TourAnchor`]: a name
//! for a component that does not depend on the layout, so the same step
//! lights the sidebar in Workspace and the top bar in Studio. The UI tells
//! which anchors are on screen; everything else (where the run is, what a
//! missing anchor turns into, what is saved) is decided here. Steps only
//! explain: a tour never creates, changes or deletes the user's data.
//!
//! Besides the welcome tour, a screen can have a tour of its own (issue
//! #107), started from its "Tour this screen" button, Shift+F1 or the
//! Guide. The button carries a "new" mark from the first visit that finds
//! something to show until the tour is completed or dismissed, and again
//! when the tour's content changes; a screen tour never starts on its own.
//! A stage of a video project has its own tour too (issue #108), behind
//! "Tour this stage", under the same rules. So does a Settings tab
//! (issue #110, Networks), and the missed posts list carries its own: the
//! one tour that runs over the list, which every other tour waits behind.

use std::collections::HashMap;
use std::time::SystemTime;

use bardo_domain::{
    ProfileId, TourId, TourProgress, TourProgressRepository, TourState, UserProfile,
};

use crate::{AppError, Bardo, Destination, Pillar, SettingsTab, Stage, Text};

/// A component a tour step can point at. The layouts tag the places and
/// parts they draw, wherever they draw them; screens tag their own
/// controls as later tours need them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TourAnchor {
    /// A pillar's places in the navigation.
    NavGroup(Pillar),
    /// One place in the navigation, pinned ones included.
    NavPlace(Destination),
    /// A screen's header: where it is, its title, its actions.
    Header,
    /// The project stages.
    Stages,
    /// Actions over the collection or the page.
    Toolbar,
    /// The items a screen holds.
    Collection,
    /// The selected item's properties, or the form that drives the
    /// collection.
    Inspector,
    /// What a page shows when it is not a collection.
    Content,
    /// The page's figures (the Render stage's length, frame, loudness and
    /// captions).
    Summary,
    /// One of a screen's own controls, tagged by the screen.
    Control(Control),
}

/// A screen's own control a tour step lights, where a part is too broad.
/// The screen tags it with `kit::anchor`, in every layout alike.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Control {
    /// Research: the channel picker and the market it sets.
    ResearchChannel,
    /// Research: the niches or keywords to look up.
    ResearchSeeds,
    /// Research: Run research and Refresh all.
    ResearchRun,
    /// Themes: the channel and niche pickers, with the niche's scores.
    ThemesPick,
    /// Themes: Suggest themes and Rank again.
    ThemesSuggest,
    /// Themes: the first idea's title, priority and confidence.
    ThemeIdea,
    /// Themes: the reasons behind the first idea's ranking.
    ThemeReasons,
    /// Themes: Approve, Edit and Discard on the first idea that has them.
    ThemeActions,
    /// Themes: the projects started from the channel's ideas.
    ThemesProjects,
    /// Projects: the open project's name and the menu that switches it.
    ProjectSwitcher,
    /// Projects: who narrates the open project.
    ProjectNarrator,
    /// Script: the script's editor, or the button that generates it.
    ScriptBody,
    /// Script: a new version waiting beside the current one.
    ScriptReview,
    /// Script: Save, Discard changes and Regenerate.
    ScriptActions,
    /// Script: what the next generation would cost.
    ScriptEstimate,
    /// Script: the current script's details and prompt.
    ScriptDetails,
    /// Script: the music prompt.
    MusicPrompt,
    /// Narration: its title, out of date mark and details.
    NarrationStatus,
    /// Narration: generating it with the narrator's voice.
    NarrationGenerate,
    /// Narration: the player and the narrated words.
    NarrationPlayer,
    /// Narration: importing a recording.
    NarrationImport,
    /// Scenes: planning them and drawing the missing images.
    ScenesPlan,
    /// Scenes: All and Pending over the scenes.
    ScenesFilter,
    /// Scenes: drawing a scene's image again, or the new one to review.
    SceneRedraw,
    /// Clips: animating every scene without a clip.
    ClipsAnimate,
    /// Clips: the picked scene's motion prompt.
    ClipMotion,
    /// Clips: the picked scene's video model.
    ClipModel,
    /// Clips: how long the scene's next clip runs and what it costs.
    ClipCost,
    /// Clips: the scene's clip, and a new one to review.
    ClipReview,
    /// Personas: the voice and the way to pick one.
    PersonaVoice,
    /// Personas: the tone and the script style.
    PersonaStyle,
    /// Personas: the generation presets and the sample.
    PersonaPresets,
    /// Personas: the realistic synthetic voice flag.
    PersonaRealistic,
    /// Personas: Save, Duplicate and Export.
    PersonaShare,
    /// Templates: one button per kind of template.
    TemplateKinds,
    /// Templates: the instructions and the prompt.
    TemplateFields,
    /// Templates: the variables a template can use.
    TemplateVariables,
    /// Templates: Save as new version, Discard and Load Bardo's default.
    TemplateActions,
    /// Editor: the preview and its transport.
    EditorPreview,
    /// Editor: the tracks, their headers and the playhead.
    EditorTracks,
    /// Editor: the Select and Split tools.
    EditorTools,
    /// Editor: Snap to words.
    EditorSnap,
    /// Editor: undo and redo.
    EditorUndo,
    /// Editor: the audio tracks' headers, with level, mute and solo.
    EditorMix,
    /// Editor: Duck music under narration.
    EditorDuck,
    /// Editor: showing the captions, and their track.
    EditorCaptions,
    /// Editor: the 16:9 and 9:16 switch over the preview.
    EditorAspect,
    /// Editor: AI cut suggestions.
    EditorSuggestions,
    /// Editor: Review & render.
    EditorRender,
    /// Render: the picked target's checks.
    RenderChecks,
    /// Render: the picked target's last file.
    RenderLast,
    /// Channels: the niche and its themes.
    ChannelNiche,
    /// Channels: the aesthetic notes.
    ChannelLook,
    /// Channels: the language and the country.
    ChannelMarket,
    /// Channels: the default persona.
    ChannelPersona,
    /// Channels: the video model and the caption style.
    ChannelDefaults,
    /// Accounts: the first network account's name, handle and preset mark.
    AccountCard,
    /// Accounts: the first network account's Edit, which opens its
    /// metadata defaults and render preset.
    AccountEdit,
    /// Accounts: the first network account's render preset line.
    AccountPreset,
    /// Accounts: the first connection's state and its buttons.
    AccountConnection,
    /// Accounts: the buttons that add the networks still free.
    AccountAdd,
    /// Settings › Networks: where the credentials are kept, and why.
    CredentialsWhy,
    /// Settings › Networks: the first network's app credentials.
    CredentialsCard,
    /// Settings › Networks: whether the first network's are saved.
    CredentialsState,
    /// Publish: the picked network's title, description and tags, or the
    /// line saying they are not written yet.
    PublishMetadata,
    /// Publish: the synthetic-content disclosure reminder.
    PublishDisclosure,
    /// Publish: the picked network's upload, or its review.
    PublishUpload,
    /// Publish: when the reviewed upload goes (or, for a TikTok draft,
    /// that it waits in the inbox).
    UploadWhen,
    /// Publish: the picked network's post, to mark as posted and link.
    PublishPost,
    /// Missed posts: the list.
    MissedList,
    /// Missed posts: the first post's Post now.
    MissedSend,
    /// Missed posts: the first post's New time.
    MissedNewTime,
    /// Missed posts: the first post's Cancel.
    MissedCancel,
    /// Missed posts: Decide later.
    MissedLater,
}

/// A place a step opens before it shows, so its anchor is on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TourPlace {
    Screen(Destination),
    /// A stage of the open video project.
    Stage(Stage),
    /// A tab of the Settings screen.
    Settings(SettingsTab),
    /// The missed posts list, over the window while it holds posts.
    Missed,
}

/// Where a step's card goes beside its lit component. A side without
/// room flips to the other one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// The side the layout leaves open around the component: right of a
    /// sidebar, below a top bar.
    Open,
    Right,
    Left,
    Below,
    Above,
}

/// What a step does when its component is not on screen (a screen with no
/// items yet, a part the layout does not draw).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhenMissing {
    /// Go on to the next step in the direction the user was going.
    Skip,
    /// Light the part that holds the component instead.
    LightPart(TourAnchor),
    /// Show the card in the middle, with nothing lit.
    Center,
}

/// A section of the user guide a step's "Learn more" opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuideRef {
    /// A page's id: its file name in `docs/guide/<language>/`.
    pub page: &'static str,
    pub section: &'static str,
}

/// One step: what it lights, where it goes first, what its card says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TourStep {
    /// Names the step's texts: `tour.<tour>.<key>.title` and `.body`.
    pub key: &'static str,
    /// `None` for a card in the middle with nothing lit.
    pub anchor: Option<TourAnchor>,
    pub place: Option<TourPlace>,
    pub side: Side,
    pub when_missing: WhenMissing,
    /// Where the guide says more, behind the card's "Learn more".
    pub guide: Option<GuideRef>,
}

impl TourStep {
    /// A step that lights `anchor`, with its card on the open side.
    const fn at(key: &'static str, anchor: TourAnchor) -> Self {
        Self {
            key,
            anchor: Some(anchor),
            place: None,
            side: Side::Open,
            when_missing: WhenMissing::Center,
            guide: None,
        }
    }

    /// A card in the middle with nothing lit.
    const fn centered(key: &'static str) -> Self {
        Self {
            key,
            anchor: None,
            place: None,
            side: Side::Open,
            when_missing: WhenMissing::Center,
            guide: None,
        }
    }

    /// The step opens `place` first, so a tour started or resumed elsewhere
    /// shows its screen or stage.
    const fn on(self, place: TourPlace) -> Self {
        Self {
            place: Some(place),
            ..self
        }
    }

    /// What the step does when its component is missing (the default
    /// shows the card in the middle).
    const fn missing(self, when_missing: WhenMissing) -> Self {
        Self {
            when_missing,
            ..self
        }
    }

    /// The step's "Learn more" opens `page` at `section`.
    const fn learn(self, page: &'static str, section: &'static str) -> Self {
        Self {
            guide: Some(GuideRef { page, section }),
            ..self
        }
    }
}

/// A tour's steps and its content version. Raise the version when the
/// steps change enough that someone who saw them should see them again: the
/// tour shows as new, and never restarts on its own.
#[derive(Debug, PartialEq, Eq)]
pub struct Tour {
    pub id: TourId,
    pub version: u32,
    /// The screen or stage it explains, whose "Tour this screen" or "Tour
    /// this stage" starts it; `None` for the welcome tour.
    pub place: Option<TourPlace>,
    pub steps: &'static [TourStep],
}

/// The first-run tour: the pillars, the keys, jobs, costs and where help
/// lives. Every step lights the navigation, which every screen shows.
pub const WELCOME: Tour = Tour {
    id: TourId::Welcome,
    version: 1,
    place: None,
    steps: &[
        TourStep::centered("intro").learn("what-bardo-is", "flow"),
        TourStep::at("strategy", TourAnchor::NavGroup(Pillar::Strategy))
            .learn("first-video", "research"),
        TourStep::at("production", TourAnchor::NavGroup(Pillar::Production))
            .learn("first-video", "project"),
        TourStep::at("publishing", TourAnchor::NavGroup(Pillar::Publishing))
            .learn("first-video", "publish"),
        TourStep::at("settings", TourAnchor::NavPlace(Destination::Settings))
            .learn("api-keys", "providers"),
        TourStep::at("jobs", TourAnchor::NavPlace(Destination::Jobs))
            .learn("what-bardo-is", "around"),
        TourStep::at("costs", TourAnchor::NavPlace(Destination::Costs))
            .learn("api-keys", "budgets"),
        TourStep::at("guide", TourAnchor::NavPlace(Destination::Guide)).learn("shortcuts", "guide"),
    ],
};

/// A step of a screen's tour: it lights `anchor` on `screen`.
const fn on(screen: Destination, key: &'static str, anchor: TourAnchor) -> TourStep {
    TourStep::at(key, anchor).on(TourPlace::Screen(screen))
}

/// A step of a stage's tour: it lights `anchor` at `stage` of the open
/// project.
const fn at_stage(stage: Stage, key: &'static str, anchor: TourAnchor) -> TourStep {
    TourStep::at(key, anchor).on(TourPlace::Stage(stage))
}

const fn control(control: Control) -> TourAnchor {
    TourAnchor::Control(control)
}

/// Research: the market, the niches, a run and its quota, the results.
pub const RESEARCH: Tour = {
    const AT: Destination = Destination::Research;
    const PAGE: &str = "niche-research";
    Tour {
        id: TourId::Research,
        version: 1,
        place: Some(TourPlace::Screen(AT)),
        steps: &[
            on(AT, "channel", TourAnchor::Control(Control::ResearchChannel))
                .missing(WhenMissing::LightPart(TourAnchor::Inspector))
                .learn(PAGE, "market"),
            on(AT, "seeds", TourAnchor::Control(Control::ResearchSeeds))
                .missing(WhenMissing::LightPart(TourAnchor::Inspector))
                .learn(PAGE, "seeds"),
            // A running job shows its progress in place of the buttons.
            on(AT, "run", TourAnchor::Control(Control::ResearchRun))
                .missing(WhenMissing::LightPart(TourAnchor::Inspector))
                .learn(PAGE, "run"),
            on(AT, "results", TourAnchor::Collection).learn(PAGE, "results"),
            on(AT, "next", TourAnchor::NavPlace(Destination::Themes)).learn(PAGE, "next"),
        ],
    }
};

/// Themes: the niche, suggesting ideas, an idea's ranking and reasons,
/// reviewing it, and the projects approved ideas start.
pub const THEMES: Tour = {
    const AT: Destination = Destination::Themes;
    const PAGE: &str = "themes-ranking";
    const IDEAS: WhenMissing = WhenMissing::LightPart(TourAnchor::Collection);
    Tour {
        id: TourId::Themes,
        version: 1,
        place: Some(TourPlace::Screen(AT)),
        steps: &[
            on(AT, "pick", TourAnchor::Control(Control::ThemesPick))
                .missing(WhenMissing::LightPart(TourAnchor::Inspector))
                .learn(PAGE, "pick"),
            on(AT, "suggest", TourAnchor::Control(Control::ThemesSuggest))
                .missing(WhenMissing::LightPart(TourAnchor::Inspector))
                .learn(PAGE, "suggest"),
            on(AT, "idea", TourAnchor::Control(Control::ThemeIdea))
                .missing(IDEAS)
                .learn(PAGE, "ranking"),
            on(AT, "reasons", TourAnchor::Control(Control::ThemeReasons))
                .missing(IDEAS)
                .learn(PAGE, "reasons"),
            on(AT, "review", TourAnchor::Control(Control::ThemeActions))
                .missing(IDEAS)
                .learn(PAGE, "review"),
            on(AT, "projects", TourAnchor::Control(Control::ThemesProjects))
                .missing(WhenMissing::LightPart(TourAnchor::Inspector))
                .learn(PAGE, "projects"),
        ],
    }
};

/// Performance: the channel's posts, a post's numbers, syncing, and
/// where posts are linked.
pub const PERFORMANCE: Tour = {
    const AT: Destination = Destination::Performance;
    const PAGE: &str = "performance-metrics";
    Tour {
        id: TourId::Performance,
        version: 1,
        place: Some(TourPlace::Screen(AT)),
        steps: &[
            on(AT, "channel", TourAnchor::Header).learn(PAGE, "channel"),
            on(AT, "posts", TourAnchor::Collection).learn(PAGE, "posts"),
            on(AT, "numbers", TourAnchor::Inspector)
                .missing(WhenMissing::Skip)
                .learn(PAGE, "numbers"),
            on(AT, "sync", TourAnchor::Toolbar)
                .missing(WhenMissing::Skip)
                .learn(PAGE, "sync"),
            on(AT, "link", TourAnchor::NavPlace(Destination::Projects)).learn(PAGE, "link"),
        ],
    }
};

/// Projects: the project and its narrator, the stages and how they open,
/// and what the project has cost.
pub const PROJECTS: Tour = {
    const AT: Destination = Destination::Projects;
    const PAGE: &str = "projects";
    const HEADER: WhenMissing = WhenMissing::LightPart(TourAnchor::Header);
    Tour {
        id: TourId::Projects,
        version: 1,
        place: Some(TourPlace::Screen(AT)),
        steps: &[
            on(AT, "switcher", control(Control::ProjectSwitcher))
                .missing(HEADER)
                .learn(PAGE, "switch"),
            on(AT, "narrator", control(Control::ProjectNarrator))
                .missing(HEADER)
                .learn(PAGE, "narrator"),
            on(AT, "stages", TourAnchor::Stages).learn(PAGE, "stages"),
            on(AT, "unlock", TourAnchor::Stages).learn(PAGE, "unlock"),
            on(AT, "cost", TourAnchor::Header).learn(PAGE, "cost"),
        ],
    }
};

/// Script: writing or generating it, a new version to review, saving,
/// what a generation costs, where the script came from, the music prompt.
pub const SCRIPT: Tour = {
    const AT: Stage = Stage::Script;
    const PAGE: &str = "script";
    const PAGE_PART: WhenMissing = WhenMissing::LightPart(TourAnchor::Content);
    Tour {
        id: TourId::Script,
        version: 1,
        place: Some(TourPlace::Stage(AT)),
        steps: &[
            at_stage(AT, "write", control(Control::ScriptBody))
                .missing(PAGE_PART)
                .learn(PAGE, "write"),
            // No new version waiting: the buttons that ask for one.
            at_stage(AT, "review", control(Control::ScriptReview))
                .missing(WhenMissing::LightPart(control(Control::ScriptActions)))
                .learn(PAGE, "review"),
            at_stage(AT, "save", control(Control::ScriptActions))
                .missing(PAGE_PART)
                .learn(PAGE, "save"),
            at_stage(AT, "cost", control(Control::ScriptEstimate))
                .missing(WhenMissing::Skip)
                .learn(PAGE, "cost"),
            at_stage(AT, "details", control(Control::ScriptDetails))
                .missing(WhenMissing::Skip)
                .learn(PAGE, "details"),
            at_stage(AT, "music", control(Control::MusicPrompt))
                .missing(WhenMissing::Skip)
                .learn(PAGE, "music"),
        ],
    }
};

/// Narration: generating it with the narrator's voice, playing it, when
/// it is out of date, importing a recording.
pub const NARRATION: Tour = {
    const AT: Stage = Stage::Narration;
    const PAGE: &str = "narration";
    const PAGE_PART: WhenMissing = WhenMissing::LightPart(TourAnchor::Content);
    Tour {
        id: TourId::Narration,
        version: 1,
        place: Some(TourPlace::Stage(AT)),
        steps: &[
            // A running job, or a narrator whose voice cannot read yet,
            // hides the button.
            at_stage(AT, "generate", control(Control::NarrationGenerate))
                .missing(PAGE_PART)
                .learn(PAGE, "generate"),
            at_stage(AT, "play", control(Control::NarrationPlayer))
                .missing(WhenMissing::Skip)
                .learn(PAGE, "play"),
            at_stage(AT, "stale", control(Control::NarrationStatus))
                .missing(PAGE_PART)
                .learn(PAGE, "stale"),
            at_stage(AT, "import", control(Control::NarrationImport))
                .missing(WhenMissing::Skip)
                .learn(PAGE, "import"),
        ],
    }
};

/// Scenes: planning them, the scene list, a scene's image and prompt,
/// drawing it again, the filter, and planning again.
pub const SCENES: Tour = {
    const AT: Stage = Stage::Scenes;
    const PAGE: &str = "scenes";
    const TOOLBAR: WhenMissing = WhenMissing::LightPart(TourAnchor::Toolbar);
    Tour {
        id: TourId::Scenes,
        version: 1,
        place: Some(TourPlace::Stage(AT)),
        steps: &[
            at_stage(AT, "plan", control(Control::ScenesPlan))
                .missing(TOOLBAR)
                .learn(PAGE, "plan"),
            at_stage(AT, "list", TourAnchor::Collection).learn(PAGE, "list"),
            // No scene picked: nothing to show beside the list.
            at_stage(AT, "scene", TourAnchor::Inspector)
                .missing(WhenMissing::Skip)
                .learn(PAGE, "scene"),
            at_stage(AT, "redraw", control(Control::SceneRedraw))
                .missing(WhenMissing::LightPart(TourAnchor::Inspector))
                .learn(PAGE, "redraw"),
            at_stage(AT, "filter", control(Control::ScenesFilter))
                .missing(WhenMissing::Skip)
                .learn(PAGE, "filter"),
            at_stage(AT, "replan", control(Control::ScenesPlan))
                .missing(TOOLBAR)
                .learn(PAGE, "replan"),
        ],
    }
};

/// Clips: animating scenes, the motion prompt, the video model, reviewing
/// a new clip, and what a clip costs.
pub const CLIPS: Tour = {
    const AT: Stage = Stage::Clips;
    const PAGE: &str = "clips";
    const SCENE: WhenMissing = WhenMissing::LightPart(TourAnchor::Inspector);
    Tour {
        id: TourId::Clips,
        version: 1,
        place: Some(TourPlace::Stage(AT)),
        steps: &[
            at_stage(AT, "animate", control(Control::ClipsAnimate))
                .missing(WhenMissing::LightPart(TourAnchor::Toolbar))
                .learn(PAGE, "animate"),
            at_stage(AT, "motion", control(Control::ClipMotion))
                .missing(SCENE)
                .learn(PAGE, "motion"),
            at_stage(AT, "model", control(Control::ClipModel))
                .missing(SCENE)
                .learn(PAGE, "model"),
            at_stage(AT, "review", control(Control::ClipReview))
                .missing(SCENE)
                .learn(PAGE, "review"),
            at_stage(AT, "cost", control(Control::ClipCost))
                .missing(WhenMissing::LightPart(control(Control::ClipModel)))
                .learn(PAGE, "cost"),
        ],
    }
};

/// Personas: the library, a persona's voice, tone and script style,
/// presets, sharing it, and the realistic voice flag.
pub const PERSONAS: Tour = {
    const AT: Destination = Destination::Personas;
    const PAGE: &str = "personas";
    const FORM: WhenMissing = WhenMissing::LightPart(TourAnchor::Inspector);
    Tour {
        id: TourId::Personas,
        version: 1,
        place: Some(TourPlace::Screen(AT)),
        steps: &[
            on(AT, "library", TourAnchor::Collection).learn(PAGE, "library"),
            on(AT, "voice", control(Control::PersonaVoice))
                .missing(FORM)
                .learn(PAGE, "voice"),
            on(AT, "style", control(Control::PersonaStyle))
                .missing(FORM)
                .learn(PAGE, "style"),
            on(AT, "presets", control(Control::PersonaPresets))
                .missing(FORM)
                .learn(PAGE, "presets"),
            on(AT, "share", control(Control::PersonaShare))
                .missing(FORM)
                .learn(PAGE, "share"),
            on(AT, "realistic", control(Control::PersonaRealistic))
                .missing(FORM)
                .learn(PAGE, "realistic"),
        ],
    }
};

/// Templates: the kinds, their versions, instructions and prompt,
/// variables, saving a version and Bardo's default.
pub const TEMPLATES: Tour = {
    const AT: Destination = Destination::Templates;
    const PAGE: &str = "templates";
    const EDITOR: WhenMissing = WhenMissing::LightPart(TourAnchor::Inspector);
    Tour {
        id: TourId::Templates,
        version: 1,
        place: Some(TourPlace::Screen(AT)),
        steps: &[
            on(AT, "kinds", control(Control::TemplateKinds))
                .missing(WhenMissing::LightPart(TourAnchor::Collection))
                .learn(PAGE, "kinds"),
            on(AT, "versions", TourAnchor::Collection).learn(PAGE, "versions"),
            on(AT, "fields", control(Control::TemplateFields))
                .missing(EDITOR)
                .learn(PAGE, "fields"),
            on(AT, "variables", control(Control::TemplateVariables))
                .missing(EDITOR)
                .learn(PAGE, "variables"),
            on(AT, "save", control(Control::TemplateActions))
                .missing(EDITOR)
                .learn(PAGE, "save"),
        ],
    }
};

/// The editor, part one: the preview and playback, the tracks, cutting,
/// snapping and undo. Starting it pauses playback; its steps only explain,
/// so the cut and its undo history stay as they were.
pub const EDITOR: Tour = {
    const AT: Stage = Stage::Edit;
    const PAGE: &str = "editor";
    Tour {
        id: TourId::Editor,
        version: 1,
        place: Some(TourPlace::Stage(AT)),
        steps: &[
            at_stage(AT, "preview", control(Control::EditorPreview)).learn(PAGE, "preview"),
            at_stage(AT, "tracks", control(Control::EditorTracks)).learn(PAGE, "timeline"),
            at_stage(AT, "tools", control(Control::EditorTools)).learn(PAGE, "cuts"),
            at_stage(AT, "snap", control(Control::EditorSnap)).learn(PAGE, "snapping"),
            at_stage(AT, "undo", control(Control::EditorUndo)).learn(PAGE, "undo"),
        ],
    }
};

/// The editor, part two: the mix, captions, framing, cut suggestions and
/// leaving to render. Offered once part one is completed or dismissed.
pub const EDITOR_MORE: Tour = {
    const AT: Stage = Stage::Edit;
    const PAGE: &str = "editor";
    Tour {
        id: TourId::EditorMore,
        version: 1,
        place: Some(TourPlace::Stage(AT)),
        steps: &[
            at_stage(AT, "mix", control(Control::EditorMix)).learn(PAGE, "mix"),
            at_stage(AT, "duck", control(Control::EditorDuck)).learn(PAGE, "ducking"),
            at_stage(AT, "captions", control(Control::EditorCaptions)).learn(PAGE, "captions"),
            at_stage(AT, "framing", control(Control::EditorAspect)).learn(PAGE, "framing"),
            at_stage(AT, "suggestions", control(Control::EditorSuggestions))
                .learn(PAGE, "suggestions"),
            // Hidden while the cut is empty.
            at_stage(AT, "render", control(Control::EditorRender))
                .missing(WhenMissing::Skip)
                .learn(PAGE, "render"),
        ],
    }
};

/// Render: the review's figures, the targets, what blocks and what warns,
/// rendering, and the last file.
pub const RENDER: Tour = {
    const AT: Stage = Stage::Render;
    const PAGE: &str = "render";
    const TARGET: WhenMissing = WhenMissing::LightPart(TourAnchor::Inspector);
    Tour {
        id: TourId::Render,
        version: 1,
        place: Some(TourPlace::Stage(AT)),
        steps: &[
            at_stage(AT, "review", TourAnchor::Summary)
                .missing(WhenMissing::LightPart(TourAnchor::Header))
                .learn(PAGE, "review"),
            at_stage(AT, "targets", TourAnchor::Collection).learn(PAGE, "targets"),
            // Still checking: the target's state stands in for them.
            at_stage(AT, "gates", control(Control::RenderChecks))
                .missing(TARGET)
                .learn(PAGE, "gates"),
            at_stage(AT, "render", TourAnchor::Toolbar).learn(PAGE, "render"),
            at_stage(AT, "last", control(Control::RenderLast))
                .missing(TARGET)
                .learn(PAGE, "last"),
        ],
    }
};

/// Channels: the list, a channel's niche and themes, its look, its market,
/// its narrator, its video model and captions, and its network accounts.
pub const CHANNELS: Tour = {
    const AT: Destination = Destination::Channels;
    const PAGE: &str = "channels";
    const FORM: WhenMissing = WhenMissing::LightPart(TourAnchor::Inspector);
    Tour {
        id: TourId::Channels,
        version: 1,
        place: Some(TourPlace::Screen(AT)),
        steps: &[
            on(AT, "list", TourAnchor::Collection).learn(PAGE, "list"),
            on(AT, "niche", control(Control::ChannelNiche))
                .missing(FORM)
                .learn(PAGE, "niche"),
            on(AT, "look", control(Control::ChannelLook))
                .missing(FORM)
                .learn(PAGE, "look"),
            on(AT, "market", control(Control::ChannelMarket))
                .missing(FORM)
                .learn(PAGE, "market"),
            on(AT, "persona", control(Control::ChannelPersona))
                .missing(FORM)
                .learn(PAGE, "persona"),
            on(AT, "defaults", control(Control::ChannelDefaults))
                .missing(FORM)
                .learn(PAGE, "defaults"),
            on(AT, "accounts", TourAnchor::NavPlace(Destination::Accounts)).learn(PAGE, "accounts"),
        ],
    }
};

/// Accounts: the channel, its network accounts, their metadata defaults
/// and render presets, connecting one and its states, and adding more.
pub const ACCOUNTS: Tour = {
    const AT: Destination = Destination::Accounts;
    const PAGE: &str = "network-accounts";
    const PANEL: WhenMissing = WhenMissing::LightPart(TourAnchor::Inspector);
    Tour {
        id: TourId::Accounts,
        version: 1,
        place: Some(TourPlace::Screen(AT)),
        steps: &[
            on(AT, "channel", TourAnchor::Collection).learn(PAGE, "channel"),
            on(AT, "accounts", control(Control::AccountCard))
                .missing(PANEL)
                .learn(PAGE, "accounts"),
            on(AT, "metadata", control(Control::AccountEdit))
                .missing(PANEL)
                .learn(PAGE, "metadata"),
            on(AT, "preset", control(Control::AccountPreset))
                .missing(PANEL)
                .learn(PAGE, "preset"),
            // X and Kick export only: nothing to connect.
            on(AT, "connect", control(Control::AccountConnection))
                .missing(WhenMissing::Skip)
                .learn(PAGE, "connect"),
            // Every network added already.
            on(AT, "add", control(Control::AccountAdd))
                .missing(WhenMissing::Skip)
                .learn(PAGE, "add"),
        ],
    }
};

/// Settings › Networks: the app credentials each network signs in with,
/// why Bardo ships none, where they are kept, and connecting after.
pub const NETWORKS: Tour = {
    const AT: TourPlace = TourPlace::Settings(SettingsTab::Networks);
    const PAGE: &str = "app-credentials";
    const CARDS: WhenMissing = WhenMissing::LightPart(TourAnchor::Content);
    Tour {
        id: TourId::Networks,
        version: 1,
        place: Some(AT),
        steps: &[
            TourStep::at("tab", TourAnchor::Toolbar)
                .on(AT)
                .learn(PAGE, "networks"),
            TourStep::at("why", control(Control::CredentialsWhy))
                .on(AT)
                .missing(CARDS)
                .learn(PAGE, "why"),
            TourStep::at("card", control(Control::CredentialsCard))
                .on(AT)
                .missing(CARDS)
                .learn(PAGE, "save"),
            TourStep::at("kept", control(Control::CredentialsState))
                .on(AT)
                .missing(CARDS)
                .learn(PAGE, "kept"),
            TourStep::at("connect", TourAnchor::NavPlace(Destination::Accounts))
                .on(AT)
                .learn(PAGE, "connect"),
        ],
    }
};

/// Publish: the networks, each one's metadata and limits, the synthetic
/// content disclosure, the upload review, scheduling, export, and the post.
/// Its steps only explain: nothing is uploaded, exported or linked.
pub const PUBLISH: Tour = {
    const AT: Stage = Stage::Publish;
    const PAGE: &str = "uploading";
    const NETWORK: WhenMissing = WhenMissing::LightPart(TourAnchor::Inspector);
    Tour {
        id: TourId::Publish,
        version: 1,
        place: Some(TourPlace::Stage(AT)),
        steps: &[
            at_stage(AT, "networks", TourAnchor::Collection).learn(PAGE, "networks"),
            at_stage(AT, "metadata", control(Control::PublishMetadata))
                .missing(NETWORK)
                .learn(PAGE, "metadata"),
            // No realistic voice: the review's disclosure stands in.
            at_stage(AT, "disclosure", control(Control::PublishDisclosure))
                .missing(WhenMissing::LightPart(control(Control::PublishUpload)))
                .learn(PAGE, "disclosure"),
            // X and Kick: exported only, so the card says it in the middle.
            at_stage(AT, "upload", control(Control::PublishUpload)).learn(PAGE, "review"),
            // The review closed: the upload stands in.
            at_stage(AT, "schedule", control(Control::UploadWhen))
                .missing(WhenMissing::LightPart(control(Control::PublishUpload)))
                .learn(PAGE, "schedule"),
            at_stage(AT, "export", TourAnchor::Toolbar).learn(PAGE, "export"),
            at_stage(AT, "post", control(Control::PublishPost))
                .missing(NETWORK)
                .learn(PAGE, "post"),
        ],
    }
};

/// The missed posts list: why a post is missed, sending it now, giving it
/// a new time, cancelling it, and deciding later. It runs over the list;
/// started with the list closed, its cards show in the middle.
pub const MISSED: Tour = {
    const AT: TourPlace = TourPlace::Missed;
    const PAGE: &str = "missed-posts";
    const LIST: WhenMissing = WhenMissing::LightPart(TourAnchor::Control(Control::MissedList));
    Tour {
        id: TourId::Missed,
        version: 1,
        place: Some(AT),
        steps: &[
            TourStep::at("list", control(Control::MissedList))
                .on(AT)
                .learn(PAGE, "why"),
            TourStep::at("send", control(Control::MissedSend))
                .on(AT)
                .missing(LIST)
                .learn(PAGE, "send"),
            TourStep::at("new-time", control(Control::MissedNewTime))
                .on(AT)
                .missing(LIST)
                .learn(PAGE, "new-time"),
            TourStep::at("cancel", control(Control::MissedCancel))
                .on(AT)
                .missing(LIST)
                .learn(PAGE, "cancel"),
            TourStep::at("later", control(Control::MissedLater))
                .on(AT)
                .missing(LIST)
                .learn(PAGE, "later"),
        ],
    }
};

impl Tour {
    /// Every tour Bardo ships.
    pub const ALL: [&'static Tour; 19] = [
        &WELCOME,
        &RESEARCH,
        &THEMES,
        &PERFORMANCE,
        &PROJECTS,
        &SCRIPT,
        &NARRATION,
        &SCENES,
        &CLIPS,
        &PERSONAS,
        &TEMPLATES,
        &EDITOR,
        &EDITOR_MORE,
        &RENDER,
        &CHANNELS,
        &ACCOUNTS,
        &NETWORKS,
        &PUBLISH,
        &MISSED,
    ];

    pub fn get(id: TourId) -> &'static Tour {
        match id {
            TourId::Welcome => &WELCOME,
            TourId::Research => &RESEARCH,
            TourId::Themes => &THEMES,
            TourId::Performance => &PERFORMANCE,
            TourId::Projects => &PROJECTS,
            TourId::Script => &SCRIPT,
            TourId::Narration => &NARRATION,
            TourId::Scenes => &SCENES,
            TourId::Clips => &CLIPS,
            TourId::Personas => &PERSONAS,
            TourId::Templates => &TEMPLATES,
            TourId::Editor => &EDITOR,
            TourId::EditorMore => &EDITOR_MORE,
            TourId::Render => &RENDER,
            TourId::Channels => &CHANNELS,
            TourId::Accounts => &ACCOUNTS,
            TourId::Networks => &NETWORKS,
            TourId::Publish => &PUBLISH,
            TourId::Missed => &MISSED,
        }
    }

    /// The tour of `place` (a screen, or a stage of a project), if it has
    /// one; the first, for a place with several parts (the editor).
    pub fn of(place: TourPlace) -> Option<&'static Tour> {
        Self::at(place).next()
    }

    /// Every tour of `place`, in order.
    pub fn at(place: TourPlace) -> impl Iterator<Item = &'static Tour> {
        Self::ALL
            .into_iter()
            .filter(move |tour| tour.place == Some(place))
    }

    /// The tour of `screen`, if it has one.
    pub fn of_screen(screen: Destination) -> Option<&'static Tour> {
        Self::of(TourPlace::Screen(screen))
    }

    /// Whether it runs over the missed posts list, which every other tour
    /// waits behind.
    pub fn over_missed_posts(&self) -> bool {
        self.place == Some(TourPlace::Missed)
    }

    pub fn len(&self) -> usize {
        self.steps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}

/// What lights up for the current step, given what is on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spot {
    Lit(TourAnchor),
    /// The card in the middle, nothing lit.
    Center,
    /// The step's component is missing and the step goes: move on with
    /// [`Bardo::tour_step_over`].
    Skip,
}

/// What the UI does after a move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TourMove {
    /// Show the run's current step, opening its place first when it has
    /// one.
    Show(Option<TourPlace>),
    /// The last step was done: the user stays where the tour left them.
    Finished,
    /// The tour was skipped: back to where the user was when it started.
    Left(Destination),
    /// The tour closed with Esc and can be resumed; nothing moves.
    Closed,
}

/// Why a tour cannot start now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TourError {
    /// The missed posts come first: tours wait until their list closes.
    #[error("the missed posts list is open")]
    MissedPostsOpen,
    #[error("there is no tour to resume")]
    NothingToResume,
}

/// The step on screen, as its card shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TourStepView {
    pub tour: TourId,
    /// From 1.
    pub number: usize,
    pub count: usize,
    pub title: Text,
    pub body: Text,
    pub side: Side,
    pub anchor: Option<TourAnchor>,
    /// Where "Learn more" leads, when the step names a guide section.
    pub guide: Option<GuideRef>,
}

impl TourStepView {
    pub fn is_first(&self) -> bool {
        self.number == 1
    }

    pub fn is_last(&self) -> bool {
        self.number == self.count
    }
}

/// A screen's tour as its "Tour this screen" button shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenTour {
    pub tour: TourId,
    /// The button carries the "new" mark.
    pub new: bool,
}

/// A tour being shown: its step, where the user was when it started, and
/// which way they were going (a missing step is passed over that way).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TourRun {
    tour: &'static Tour,
    step: usize,
    origin: Destination,
    forward: bool,
}

impl TourRun {
    fn new(tour: &'static Tour, step: usize, origin: Destination) -> Self {
        Self {
            tour,
            step: step.min(tour.len().saturating_sub(1)),
            origin,
            forward: true,
        }
    }

    fn current(&self) -> &'static TourStep {
        &self.tour.steps[self.step]
    }

    fn shown(&self) -> TourMove {
        TourMove::Show(self.current().place)
    }

    /// `None` once past the last step.
    fn next(&mut self) -> Option<TourMove> {
        self.forward = true;
        if self.step + 1 >= self.tour.len() {
            return None;
        }
        self.step += 1;
        Some(self.shown())
    }

    /// Stays on the first step.
    fn back(&mut self) -> TourMove {
        self.forward = false;
        self.step = self.step.saturating_sub(1);
        self.shown()
    }

    /// Passes over a step whose component is missing, the way the user was
    /// going; at the first step it turns forward. `None` once past the
    /// last step.
    fn step_over(&mut self) -> Option<TourMove> {
        if !self.forward && self.step > 0 {
            Some(self.back())
        } else {
            self.next()
        }
    }

    fn spot(&self, on_screen: impl Fn(TourAnchor) -> bool) -> Spot {
        let step = self.current();
        let Some(anchor) = step.anchor else {
            return Spot::Center;
        };
        if on_screen(anchor) {
            return Spot::Lit(anchor);
        }
        match step.when_missing {
            WhenMissing::Skip => Spot::Skip,
            WhenMissing::LightPart(part) if on_screen(part) => Spot::Lit(part),
            WhenMissing::LightPart(_) | WhenMissing::Center => Spot::Center,
        }
    }

    fn view(&self) -> TourStepView {
        let step = self.current();
        TourStepView {
            tour: self.tour.id,
            number: self.step + 1,
            count: self.tour.len(),
            title: Text::TourStepTitle(self.tour.id, step.key),
            body: Text::TourStepBody(self.tour.id, step.key),
            side: step.side,
            anchor: step.anchor,
            guide: step.guide,
        }
    }
}

/// The profile's tour progress and the tour on screen.
pub(crate) struct TourBook {
    owner: ProfileId,
    repository: Box<dyn TourProgressRepository>,
    progress: HashMap<TourId, TourProgress>,
    run: Option<TourRun>,
    /// The first-run offer was answered (or a tour started) this session.
    offer_answered: bool,
}

impl TourBook {
    /// Reads the profile's progress. Progress is a convenience: when it
    /// cannot be read, every tour starts as never seen and the failure is
    /// logged, rather than Bardo not opening.
    pub(crate) fn load(owner: ProfileId, repository: Box<dyn TourProgressRepository>) -> Self {
        let progress = match repository.tour_progress(owner) {
            Ok(progress) => progress
                .into_iter()
                .map(|progress| (progress.tour, progress))
                .collect(),
            Err(error) => {
                tracing::warn!(%error, "could not read tour progress");
                HashMap::new()
            }
        };
        Self {
            owner,
            repository,
            progress,
            run: None,
            offer_answered: false,
        }
    }

    /// Remembers `state` at `step` of `tour`. Progress is a convenience:
    /// when it cannot be saved, the tour goes on and the failure is logged.
    fn record(&mut self, tour: &Tour, state: TourState, step: usize) {
        let progress = TourProgress {
            profile: self.owner,
            tour: tour.id,
            version: tour.version,
            state,
            last_step: u32::try_from(step).unwrap_or(u32::MAX),
            updated_at: SystemTime::now(),
        };
        if let Err(error) = self.repository.save_tour_progress(&progress) {
            tracing::warn!(%error, tour = tour.id.code(), "could not save tour progress");
        }
        self.progress.insert(tour.id, progress);
    }

    fn record_run(&mut self) {
        if let Some(run) = self.run {
            self.record(run.tour, TourState::InProgress, run.step);
        }
    }
}

impl Bardo {
    /// Whether to ask "Take a 2-minute tour?": the welcome tour was never
    /// started nor turned down for good, and nothing answered it in this
    /// session.
    pub fn tour_offer(&self) -> bool {
        let book = &self.tours;
        !book.offer_answered
            && book.run.is_none()
            && book
                .progress
                .get(&TourId::Welcome)
                .is_none_or(|progress| progress.state == TourState::Offered)
    }

    /// "Not now" (`never` false): asked again next time Bardo opens.
    /// "Don't show again" (`never` true): never asked again; the tour stays
    /// in the Guide menu.
    pub fn decline_tour_offer(&mut self, never: bool) {
        let book = &mut self.tours;
        book.offer_answered = true;
        let state = if never {
            TourState::Dismissed
        } else {
            TourState::Offered
        };
        book.record(&WELCOME, state, 0);
    }

    /// Starts `tour` from its first step; `from` is where Skip returns.
    /// A tour already on screen closes first, keeping its step.
    pub fn start_tour(
        &mut self,
        tour: TourId,
        from: Destination,
        missed_posts_open: bool,
    ) -> Result<TourMove, TourError> {
        self.begin_tour(Tour::get(tour), 0, from, missed_posts_open)
    }

    /// The tour closed midway, if any: "Resume tour" goes back to it.
    /// With several, the one closed last.
    pub fn resumable_tour(&self) -> Option<TourId> {
        let running = self.tours.run.map(|run| run.tour.id);
        self.tours
            .progress
            .values()
            .filter(|progress| {
                progress.state == TourState::InProgress && Some(progress.tour) != running
            })
            .max_by_key(|progress| (progress.updated_at, progress.tour.code()))
            .map(|progress| progress.tour)
    }

    /// Goes back to the step the closed tour was on. A tour whose content
    /// changed since starts over.
    pub fn resume_tour(
        &mut self,
        from: Destination,
        missed_posts_open: bool,
    ) -> Result<TourMove, TourError> {
        let id = self.resumable_tour().ok_or(TourError::NothingToResume)?;
        let tour = Tour::get(id);
        self.begin_tour(tour, self.closed_step(tour), from, missed_posts_open)
    }

    /// Starts `tour`, or takes it back to the step it closed on when it
    /// closed midway: the editor's tour button, where the Guide's menu with
    /// Resume tour is out of reach.
    pub fn continue_tour(
        &mut self,
        tour: TourId,
        from: Destination,
        missed_posts_open: bool,
    ) -> Result<TourMove, TourError> {
        let tour = Tour::get(tour);
        let closed = self
            .tours
            .progress
            .get(&tour.id)
            .is_some_and(|progress| progress.state == TourState::InProgress);
        let step = if closed { self.closed_step(tour) } else { 0 };
        self.begin_tour(tour, step, from, missed_posts_open)
    }

    /// The step `tour` closed on; the first when its content changed since.
    fn closed_step(&self, tour: &Tour) -> usize {
        self.tours
            .progress
            .get(&tour.id)
            .filter(|progress| progress.version == tour.version)
            .map_or(0, |progress| progress.last_step as usize)
    }

    fn begin_tour(
        &mut self,
        tour: &'static Tour,
        step: usize,
        from: Destination,
        missed_posts_open: bool,
    ) -> Result<TourMove, TourError> {
        if missed_posts_open && !tour.over_missed_posts() {
            return Err(TourError::MissedPostsOpen);
        }
        let book = &mut self.tours;
        book.record_run();
        book.offer_answered = true;
        let run = TourRun::new(tour, step, from);
        book.run = Some(run);
        book.record_run();
        Ok(run.shown())
    }

    /// The step on screen; `None` without a tour, or while the missed
    /// posts list is open (the tour waits behind it), unless the tour is
    /// the list's own.
    pub fn tour_step(&self, missed_posts_open: bool) -> Option<TourStepView> {
        self.tours
            .run
            .filter(|run| !missed_posts_open || run.tour.over_missed_posts())
            .map(|run| run.view())
    }

    /// What lights up for the step on screen, given the anchors on screen.
    pub fn tour_spot(&self, on_screen: impl Fn(TourAnchor) -> bool) -> Option<Spot> {
        self.tours.run.map(|run| run.spot(on_screen))
    }

    /// Next, or Finish on the last step.
    pub fn tour_next(&mut self) -> TourMove {
        let book = &mut self.tours;
        let Some(mut run) = book.run else {
            return TourMove::Closed;
        };
        match run.next() {
            Some(moved) => {
                book.run = Some(run);
                book.record_run();
                moved
            }
            None => self.finish_tour(run),
        }
    }

    pub fn tour_back(&mut self) -> TourMove {
        let book = &mut self.tours;
        let Some(mut run) = book.run else {
            return TourMove::Closed;
        };
        let moved = run.back();
        book.run = Some(run);
        book.record_run();
        moved
    }

    /// Passes over a step whose component is not on screen ([`Spot::Skip`]).
    pub fn tour_step_over(&mut self) -> TourMove {
        let book = &mut self.tours;
        let Some(mut run) = book.run else {
            return TourMove::Closed;
        };
        match run.step_over() {
            Some(moved) => {
                book.run = Some(run);
                book.record_run();
                moved
            }
            None => self.finish_tour(run),
        }
    }

    fn finish_tour(&mut self, run: TourRun) -> TourMove {
        let book = &mut self.tours;
        book.run = None;
        book.record(run.tour, TourState::Completed, run.step);
        TourMove::Finished
    }

    /// Skip: the tour ends for good (no "Resume tour") and the user goes
    /// back to where they were.
    pub fn tour_skip(&mut self) -> TourMove {
        let book = &mut self.tours;
        let Some(run) = book.run.take() else {
            return TourMove::Closed;
        };
        book.record(run.tour, TourState::Dismissed, run.step);
        TourMove::Left(run.origin)
    }

    /// Esc: the tour closes and keeps its step for "Resume tour".
    pub fn tour_close(&mut self) -> TourMove {
        let book = &mut self.tours;
        if let Some(run) = book.run.take() {
            book.record(run.tour, TourState::InProgress, run.step);
        }
        TourMove::Closed
    }

    /// Whether `tour` reads as new: never started (only offered at most),
    /// or its content changed since the user last saw it.
    pub fn tour_is_new(&self, tour: TourId) -> bool {
        self.tours.progress.get(&tour).is_none_or(|progress| {
            progress.state == TourState::Offered || progress.version < Tour::get(tour).version
        })
    }

    /// The tour `screen` offers ("Tour this screen"), when it has one and
    /// something to show (`has_content`: research results, ideas,
    /// publications). A screen with nothing yet offers none: its empty
    /// state says what to do. The button is marked new while the tour,
    /// at its current content, was neither completed nor dismissed, unless
    /// the profile turned the mark off.
    pub fn screen_tour(&self, screen: Destination, has_content: bool) -> Option<ScreenTour> {
        self.place_tour(TourPlace::Screen(screen), has_content)
    }

    /// The tour `stage` of the open project offers ("Tour this stage"),
    /// under the same rules as a screen's: only once the stage has made
    /// something (a script, a narration, scenes, a clip, a cut).
    pub fn stage_tour(&self, stage: Stage, has_content: bool) -> Option<ScreenTour> {
        self.place_tour(TourPlace::Stage(stage), has_content)
    }

    /// The tour `place` offers (a Settings tab's, the missed posts list's),
    /// under the same rules as a screen's. A place with several tours (the
    /// editor's two parts) offers the first one the user has not completed
    /// nor dismissed, else the first again.
    pub fn place_tour(&self, place: TourPlace, has_content: bool) -> Option<ScreenTour> {
        if !has_content {
            return None;
        }
        let first = Tour::of(place)?;
        let open = Tour::at(place).find(|tour| !self.tour_done(tour));
        Some(ScreenTour {
            tour: open.unwrap_or(first).id,
            new: self.profile.offer_screen_tours && open.is_some(),
        })
    }

    /// Whether `tour`, at its current content, was completed or dismissed.
    fn tour_done(&self, tour: &Tour) -> bool {
        self.tours.progress.get(&tour.id).is_some_and(|progress| {
            progress.version >= tour.version
                && matches!(progress.state, TourState::Completed | TourState::Dismissed)
        })
    }

    /// Whether "Tour this screen" is marked new on screens whose tour the
    /// user has not taken ("Offer tours on new screens").
    pub fn offers_screen_tours(&self) -> bool {
        self.profile.offer_screen_tours
    }

    /// Turns the "new" mark on screens' tours on or off and remembers it.
    /// On failure the current setting stays.
    pub fn set_offer_screen_tours(&mut self, on: bool) -> Result<(), AppError> {
        if on == self.profile.offer_screen_tours {
            return Ok(());
        }
        let updated = UserProfile {
            offer_screen_tours: on,
            ..self.profile.clone()
        };
        self.profiles.save(&updated)?;
        self.profile = updated;
        Ok(())
    }

    /// "Reset tours": every tour reads as never seen, and the welcome offer
    /// comes back at once.
    pub fn reset_tours(&mut self) -> Result<(), AppError> {
        let book = &mut self.tours;
        book.repository.reset_tour_progress(book.owner)?;
        book.progress.clear();
        book.run = None;
        book.offer_answered = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;

    use bardo_domain::RepositoryError;
    use bardo_storage::Database;

    use super::*;
    use crate::{Catalog, Repositories, testing};

    /// Keeps progress in memory, shared with the test, and can fail.
    #[derive(Clone, Default)]
    struct FakeProgress {
        rows: Rc<RefCell<Vec<TourProgress>>>,
        fail: Rc<RefCell<bool>>,
    }

    impl FakeProgress {
        fn state(&self, tour: TourId) -> Option<(TourState, u32)> {
            self.rows
                .borrow()
                .iter()
                .find(|row| row.tour == tour)
                .map(|row| (row.state, row.last_step))
        }
    }

    impl TourProgressRepository for FakeProgress {
        fn tour_progress(&self, _: ProfileId) -> Result<Vec<TourProgress>, RepositoryError> {
            if *self.fail.borrow() {
                return Err(RepositoryError(std::io::Error::other("unreadable").into()));
            }
            Ok(self.rows.borrow().clone())
        }

        fn save_tour_progress(&self, progress: &TourProgress) -> Result<(), RepositoryError> {
            if *self.fail.borrow() {
                return Err(RepositoryError(std::io::Error::other("disk full").into()));
            }
            let mut rows = self.rows.borrow_mut();
            rows.retain(|row| row.tour != progress.tour);
            rows.push(progress.clone());
            Ok(())
        }

        fn reset_tour_progress(&self, _: ProfileId) -> Result<(), RepositoryError> {
            self.rows.borrow_mut().clear();
            Ok(())
        }
    }

    fn start(progress: &FakeProgress) -> Bardo {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let repositories = Repositories {
            tours: Box::new(progress.clone()),
            ..Repositories::shared(db, Arc::new(bardo_storage::MemorySecretStore::default()))
        };
        Bardo::start(repositories, testing::providers(), Some("en-US")).unwrap()
    }

    fn at(app: &Bardo) -> usize {
        app.tour_step(false).map_or(0, |step| step.number)
    }

    // A tour with every kind of step, for the run's rules.
    const LIT: TourStep = TourStep::at("lit", TourAnchor::Header);
    const PLACED: TourStep = TourStep {
        place: Some(TourPlace::Screen(Destination::Research)),
        ..TourStep::at("placed", TourAnchor::Collection)
    };
    const SKIPPED: TourStep = TourStep {
        when_missing: WhenMissing::Skip,
        ..TourStep::at("skipped", TourAnchor::Toolbar)
    };
    const PART: TourStep = TourStep {
        when_missing: WhenMissing::LightPart(TourAnchor::Inspector),
        ..TourStep::at("part", TourAnchor::NavPlace(Destination::Costs))
    };
    static SAMPLE: Tour = Tour {
        id: TourId::Welcome,
        version: 1,
        place: None,
        steps: &[LIT, SKIPPED, PLACED, PART],
    };

    #[test]
    fn next_walks_the_steps_and_finishes_after_the_last() {
        let mut run = TourRun::new(&SAMPLE, 0, Destination::Projects);
        assert_eq!(run.next(), Some(TourMove::Show(None)));
        assert_eq!(
            run.next(),
            Some(TourMove::Show(Some(TourPlace::Screen(
                Destination::Research
            ))))
        );
        assert_eq!(run.next(), Some(TourMove::Show(None)));
        assert_eq!(run.next(), None);
        assert_eq!(run.step, 3);
    }

    #[test]
    fn back_stays_on_the_first_step() {
        let mut run = TourRun::new(&SAMPLE, 1, Destination::Projects);
        run.back();
        assert_eq!(run.step, 0);
        run.back();
        assert_eq!(run.step, 0);
    }

    #[test]
    fn a_missing_step_is_passed_over_the_way_the_user_was_going() {
        let mut run = TourRun::new(&SAMPLE, 0, Destination::Projects);
        run.next();
        assert_eq!(run.spot(|_| false), Spot::Skip);
        run.step_over();
        assert_eq!(run.step, 2, "forward");

        run.back();
        assert_eq!(run.step, 1);
        run.step_over();
        assert_eq!(run.step, 0, "backward");
    }

    #[test]
    fn a_missing_first_step_turns_forward() {
        static FIRST_MISSING: Tour = Tour {
            id: TourId::Welcome,
            version: 1,
            place: None,
            steps: &[SKIPPED, LIT],
        };
        let mut run = TourRun::new(&FIRST_MISSING, 1, Destination::Projects);
        run.back();
        assert_eq!(run.step_over(), Some(TourMove::Show(None)));
        assert_eq!(run.step, 1);
    }

    #[test]
    fn a_missing_last_step_finishes() {
        static LAST_MISSING: Tour = Tour {
            id: TourId::Welcome,
            version: 1,
            place: None,
            steps: &[LIT, SKIPPED],
        };
        let mut run = TourRun::new(&LAST_MISSING, 1, Destination::Projects);
        assert_eq!(run.step_over(), None);
    }

    #[test]
    fn a_missing_component_lights_its_part_or_centers_the_card() {
        let mut run = TourRun::new(&SAMPLE, 3, Destination::Projects);
        assert_eq!(
            run.spot(|anchor| anchor == TourAnchor::NavPlace(Destination::Costs)),
            Spot::Lit(TourAnchor::NavPlace(Destination::Costs))
        );
        assert_eq!(
            run.spot(|anchor| anchor == TourAnchor::Inspector),
            Spot::Lit(TourAnchor::Inspector)
        );
        assert_eq!(run.spot(|_| false), Spot::Center);

        run.step = 0;
        assert_eq!(run.spot(|_| false), Spot::Center, "the default");
        run.step = 2;
        assert_eq!(run.spot(|_| true), Spot::Lit(TourAnchor::Collection));
    }

    #[test]
    fn the_welcome_tour_has_eight_steps_over_the_navigation() {
        assert_eq!(WELCOME.len(), 8);
        assert_eq!(WELCOME.steps[0].anchor, None, "a welcome in the middle");
        assert_eq!(
            WELCOME.steps[7].anchor,
            Some(TourAnchor::NavPlace(Destination::Guide))
        );
        for step in WELCOME.steps {
            assert_eq!(step.place, None, "{}: opens nothing", step.key);
            assert!(
                matches!(
                    step.anchor,
                    None | Some(TourAnchor::NavGroup(_) | TourAnchor::NavPlace(_))
                ),
                "{}: lights the navigation, on every screen",
                step.key
            );
        }
    }

    #[test]
    fn every_step_has_its_texts_in_every_language() {
        for language in bardo_domain::UiLanguage::ALL {
            let catalog = Catalog::load(language);
            for tour in Tour::ALL {
                for step in tour.steps {
                    for text in [
                        Text::TourStepTitle(tour.id, step.key),
                        Text::TourStepBody(tour.id, step.key),
                    ] {
                        assert!(catalog.has(text), "{language:?}: {text:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn step_keys_are_unique_in_a_tour() {
        for tour in Tour::ALL {
            let mut keys: Vec<_> = tour.steps.iter().map(|step| step.key).collect();
            keys.sort_unstable();
            keys.dedup();
            assert_eq!(keys.len(), tour.len(), "{:?}", tour.id);
        }
    }

    #[test]
    fn the_first_run_offers_the_welcome_tour() {
        let app = start(&FakeProgress::default());
        assert!(app.tour_offer());
        assert!(app.tour_is_new(TourId::Welcome));
        assert_eq!(app.tour_step(false), None);
    }

    #[test]
    fn not_now_asks_again_next_time() {
        let progress = FakeProgress::default();
        let mut app = start(&progress);
        app.decline_tour_offer(false);
        assert!(!app.tour_offer(), "not again in this session");
        assert_eq!(
            progress.state(TourId::Welcome),
            Some((TourState::Offered, 0))
        );

        let app = start(&progress);
        assert!(app.tour_offer());
        assert!(app.tour_is_new(TourId::Welcome), "offered is not seen");
    }

    #[test]
    fn dont_show_again_is_never_asked_again() {
        let progress = FakeProgress::default();
        start(&progress).decline_tour_offer(true);
        let app = start(&progress);
        assert!(!app.tour_offer());
        assert!(!app.tour_is_new(TourId::Welcome), "seen, so not new");
    }

    #[test]
    fn starting_shows_the_first_step_and_ends_the_offer() {
        let progress = FakeProgress::default();
        let mut app = start(&progress);
        let moved = app
            .start_tour(TourId::Welcome, Destination::Projects, false)
            .unwrap();
        assert_eq!(moved, TourMove::Show(None));
        let step = app.tour_step(false).unwrap();
        assert_eq!((step.number, step.count), (1, 8));
        assert!(step.is_first() && !step.is_last());
        assert_eq!(step.title, Text::TourStepTitle(TourId::Welcome, "intro"));
        assert!(!app.tour_offer());
        assert_eq!(
            progress.state(TourId::Welcome),
            Some((TourState::InProgress, 0))
        );
        assert!(!start(&progress).tour_offer(), "started is answered");
    }

    #[test]
    fn next_and_back_move_and_save_the_step() {
        let progress = FakeProgress::default();
        let mut app = start(&progress);
        app.start_tour(TourId::Welcome, Destination::Projects, false)
            .unwrap();
        app.tour_next();
        app.tour_next();
        assert_eq!(at(&app), 3);
        app.tour_back();
        assert_eq!(at(&app), 2);
        assert_eq!(
            progress.state(TourId::Welcome),
            Some((TourState::InProgress, 1))
        );
    }

    #[test]
    fn finish_completes_the_tour_and_stays_put() {
        let progress = FakeProgress::default();
        let mut app = start(&progress);
        app.start_tour(TourId::Welcome, Destination::Costs, false)
            .unwrap();
        for _ in 1..WELCOME.len() {
            assert!(matches!(app.tour_next(), TourMove::Show(_)));
        }
        assert!(app.tour_step(false).unwrap().is_last());
        assert_eq!(app.tour_next(), TourMove::Finished);
        assert_eq!(app.tour_step(false), None);
        assert_eq!(
            progress.state(TourId::Welcome),
            Some((TourState::Completed, 7))
        );
        assert_eq!(app.resumable_tour(), None);
        assert!(!app.tour_is_new(TourId::Welcome));
    }

    #[test]
    fn skip_goes_back_where_the_user_was_and_ends_the_tour() {
        let progress = FakeProgress::default();
        let mut app = start(&progress);
        app.start_tour(TourId::Welcome, Destination::Themes, false)
            .unwrap();
        app.tour_next();
        assert_eq!(app.tour_skip(), TourMove::Left(Destination::Themes));
        assert_eq!(app.tour_step(false), None);
        assert_eq!(app.resumable_tour(), None);
        assert_eq!(
            progress.state(TourId::Welcome),
            Some((TourState::Dismissed, 1))
        );
    }

    #[test]
    fn esc_keeps_the_step_for_resume_tour() {
        let progress = FakeProgress::default();
        let mut app = start(&progress);
        app.start_tour(TourId::Welcome, Destination::Projects, false)
            .unwrap();
        app.tour_next();
        app.tour_next();
        assert_eq!(app.tour_close(), TourMove::Closed);
        assert_eq!(app.tour_step(false), None);
        assert_eq!(app.resumable_tour(), Some(TourId::Welcome));

        // After a restart too.
        let mut app = start(&progress);
        assert!(!app.tour_offer());
        assert_eq!(app.resumable_tour(), Some(TourId::Welcome));
        app.resume_tour(Destination::Personas, false).unwrap();
        assert_eq!(at(&app), 3);
        assert_eq!(app.resumable_tour(), None, "it is on screen");
        assert_eq!(app.tour_skip(), TourMove::Left(Destination::Personas));
    }

    #[test]
    fn resume_tour_goes_back_to_the_tour_closed_last() {
        let progress = FakeProgress::default();
        let mut app = start(&progress);
        app.start_tour(TourId::Welcome, Destination::Projects, false)
            .unwrap();
        app.tour_next();
        assert_eq!(app.tour_close(), TourMove::Closed);
        app.start_tour(TourId::Research, Destination::Research, false)
            .unwrap();
        app.tour_next();
        assert_eq!(app.tour_close(), TourMove::Closed);
        assert_eq!(app.resumable_tour(), Some(TourId::Research));

        app.resume_tour(Destination::Research, false).unwrap();
        assert_eq!(app.resumable_tour(), Some(TourId::Welcome), "the other one");
    }

    #[test]
    fn nothing_to_resume_without_a_closed_tour() {
        let mut app = start(&FakeProgress::default());
        assert_eq!(
            app.resume_tour(Destination::Projects, false),
            Err(TourError::NothingToResume)
        );
    }

    #[test]
    fn a_tour_whose_content_changed_resumes_from_the_start() {
        let progress = FakeProgress::default();
        progress.rows.borrow_mut().push(TourProgress {
            profile: ProfileId::new(),
            tour: TourId::Welcome,
            version: 0,
            state: TourState::InProgress,
            last_step: 5,
            updated_at: SystemTime::UNIX_EPOCH,
        });
        let mut app = start(&progress);
        assert!(app.tour_is_new(TourId::Welcome), "new content");
        assert!(!app.tour_offer(), "it never restarts on its own");
        app.resume_tour(Destination::Projects, false).unwrap();
        assert_eq!(at(&app), 1);
    }

    #[test]
    fn new_content_shows_a_finished_tour_as_new_without_offering_it() {
        let progress = FakeProgress::default();
        progress.rows.borrow_mut().push(TourProgress {
            profile: ProfileId::new(),
            tour: TourId::Welcome,
            version: 0,
            state: TourState::Completed,
            last_step: 7,
            updated_at: SystemTime::UNIX_EPOCH,
        });
        let app = start(&progress);
        assert!(app.tour_is_new(TourId::Welcome));
        assert!(!app.tour_offer());
        assert_eq!(app.tour_step(false), None);
    }

    #[test]
    fn tours_wait_for_the_missed_posts_list() {
        let mut app = start(&FakeProgress::default());
        assert_eq!(
            app.start_tour(TourId::Welcome, Destination::Projects, true),
            Err(TourError::MissedPostsOpen)
        );
        assert_eq!(app.tour_step(false), None);

        app.start_tour(TourId::Welcome, Destination::Projects, false)
            .unwrap();
        app.tour_next();
        assert_eq!(app.tour_step(true), None, "paused behind the list");
        assert_eq!(at(&app), 2, "and back where it was");
    }

    #[test]
    fn starting_another_tour_keeps_the_running_ones_step() {
        let progress = FakeProgress::default();
        let mut app = start(&progress);
        app.start_tour(TourId::Welcome, Destination::Projects, false)
            .unwrap();
        app.tour_next();
        app.start_tour(TourId::Welcome, Destination::Projects, false)
            .unwrap();
        assert_eq!(at(&app), 1);
    }

    #[test]
    fn a_failed_save_does_not_stop_the_tour() {
        let progress = FakeProgress::default();
        let mut app = start(&progress);
        *progress.fail.borrow_mut() = true;
        app.start_tour(TourId::Welcome, Destination::Projects, false)
            .unwrap();
        app.tour_next();
        assert_eq!(at(&app), 2);
        assert_eq!(progress.state(TourId::Welcome), None);
    }

    #[test]
    fn unreadable_progress_still_opens_bardo() {
        let progress = FakeProgress::default();
        start(&progress).decline_tour_offer(true);
        *progress.fail.borrow_mut() = true;
        let app = start(&progress);
        assert!(app.tour_offer(), "every tour as never seen");
        assert!(app.tour_is_new(TourId::Welcome));
    }

    #[test]
    fn reset_forgets_every_tour_and_offers_again() {
        let progress = FakeProgress::default();
        let mut app = start(&progress);
        app.start_tour(TourId::Welcome, Destination::Projects, false)
            .unwrap();
        app.tour_close();
        app.reset_tours().unwrap();
        assert!(app.tour_offer());
        assert_eq!(app.resumable_tour(), None);
        assert!(app.tour_is_new(TourId::Welcome));
        assert_eq!(progress.state(TourId::Welcome), None);
    }

    fn saved(progress: &FakeProgress, tour: TourId, version: u32, state: TourState) {
        progress.rows.borrow_mut().push(TourProgress {
            profile: ProfileId::new(),
            tour,
            version,
            state,
            last_step: 0,
            updated_at: SystemTime::UNIX_EPOCH,
        });
    }

    fn mark(app: &Bardo, screen: Destination) -> Option<bool> {
        app.screen_tour(screen, true).map(|tour| tour.new)
    }

    #[test]
    fn each_screen_and_stage_with_a_tour_finds_it() {
        let screens = [
            (Destination::Research, TourId::Research),
            (Destination::Themes, TourId::Themes),
            (Destination::Performance, TourId::Performance),
            (Destination::Projects, TourId::Projects),
            (Destination::Personas, TourId::Personas),
            (Destination::Templates, TourId::Templates),
            (Destination::Channels, TourId::Channels),
            (Destination::Accounts, TourId::Accounts),
        ];
        for (screen, tour) in screens {
            assert_eq!(Tour::of_screen(screen).map(|t| t.id), Some(tour));
        }
        let stages = [
            (Stage::Script, TourId::Script),
            (Stage::Narration, TourId::Narration),
            (Stage::Scenes, TourId::Scenes),
            (Stage::Clips, TourId::Clips),
            (Stage::Edit, TourId::Editor),
            (Stage::Render, TourId::Render),
            (Stage::Publish, TourId::Publish),
        ];
        for (stage, tour) in stages {
            assert_eq!(Tour::of(TourPlace::Stage(stage)).map(|t| t.id), Some(tour));
        }
        assert_eq!(Tour::of_screen(Destination::Costs), None, "not yet");
        assert_eq!(
            Tour::of(TourPlace::Settings(SettingsTab::Networks)).map(|t| t.id),
            Some(TourId::Networks)
        );
        assert_eq!(
            Tour::of(TourPlace::Settings(SettingsTab::Keys)),
            None,
            "not yet"
        );
        assert_eq!(
            Tour::of(TourPlace::Missed).map(|t| t.id),
            Some(TourId::Missed)
        );
        let editor: Vec<_> = Tour::at(TourPlace::Stage(Stage::Edit))
            .map(|tour| tour.id)
            .collect();
        assert_eq!(editor, [TourId::Editor, TourId::EditorMore], "in two parts");
        for tour in Tour::ALL {
            assert_eq!(Tour::get(tour.id), tour);
        }
        let mut ids: Vec<_> = Tour::ALL.iter().map(|tour| tour.id.code()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), TourId::ALL.len(), "every tour, once");
    }

    #[test]
    fn place_tours_have_three_to_seven_steps_on_their_place() {
        for tour in Tour::ALL {
            let Some(place) = tour.place else {
                continue;
            };
            assert!((3..=7).contains(&tour.len()), "{:?}", tour.id);
            for step in tour.steps {
                assert_eq!(
                    step.place,
                    Some(place),
                    "{:?} {}: opens its place, so a resumed tour shows it",
                    tour.id,
                    step.key
                );
                assert!(
                    step.guide.is_some(),
                    "{:?} {}: learn more",
                    tour.id,
                    step.key
                );
            }
        }
    }

    #[test]
    fn a_stage_offers_its_tour_once_it_has_made_something() {
        let progress = FakeProgress::default();
        let mut app = start(&progress);
        assert_eq!(app.stage_tour(Stage::Script, false), None, "no script yet");
        assert_eq!(app.stage_tour(Stage::Edit, false), None, "no cut yet");
        assert_eq!(
            app.stage_tour(Stage::Scenes, true),
            Some(ScreenTour {
                tour: TourId::Scenes,
                new: true
            })
        );
        app.start_tour(TourId::Scenes, Destination::Projects, false)
            .unwrap();
        assert_eq!(app.tour_skip(), TourMove::Left(Destination::Projects));
        assert_eq!(
            app.stage_tour(Stage::Scenes, true).map(|tour| tour.new),
            Some(false),
            "dismissed"
        );
        assert_eq!(
            app.stage_tour(Stage::Clips, true).map(|tour| tour.new),
            Some(true),
            "each stage keeps its own"
        );
    }

    #[test]
    fn the_editor_offers_its_second_part_once_the_first_is_done() {
        let progress = FakeProgress::default();
        let mut app = start(&progress);
        let offered = |app: &Bardo| app.stage_tour(Stage::Edit, true);
        assert_eq!(app.stage_tour(Stage::Edit, false), None, "no cut yet");
        assert_eq!(
            offered(&app),
            Some(ScreenTour {
                tour: TourId::Editor,
                new: true
            })
        );
        // Closed midway: part one is still the one offered.
        app.start_tour(TourId::Editor, Destination::Projects, false)
            .unwrap();
        app.tour_close();
        assert_eq!(offered(&app).map(|tour| tour.tour), Some(TourId::Editor));
        app.start_tour(TourId::Editor, Destination::Projects, false)
            .unwrap();
        while app.tour_next() != TourMove::Finished {}
        assert_eq!(
            offered(&app),
            Some(ScreenTour {
                tour: TourId::EditorMore,
                new: true
            }),
            "part two, still new"
        );
        app.start_tour(TourId::EditorMore, Destination::Projects, false)
            .unwrap();
        app.tour_skip();
        assert_eq!(
            offered(&app),
            Some(ScreenTour {
                tour: TourId::Editor,
                new: false
            }),
            "both done: part one again, without the mark"
        );
    }

    #[test]
    fn the_editor_button_takes_a_closed_tour_back_to_its_step() {
        let progress = FakeProgress::default();
        let mut app = start(&progress);
        let from = Destination::Projects;
        let step = |app: &Bardo| app.tour_step(false).map(|step| step.number);
        app.continue_tour(TourId::Editor, from, false).unwrap();
        assert_eq!(step(&app), Some(1), "never run: from the start");
        app.tour_next();
        app.tour_next();
        app.tour_close();
        app.continue_tour(TourId::Editor, from, false).unwrap();
        assert_eq!(step(&app), Some(3), "closed on the third step");
        while app.tour_next() != TourMove::Finished {}
        app.continue_tour(TourId::Editor, from, false).unwrap();
        assert_eq!(step(&app), Some(1), "done: from the start again");
    }

    #[test]
    fn a_stage_tour_opens_its_stage_first() {
        let mut app = start(&FakeProgress::default());
        assert_eq!(
            app.start_tour(TourId::Narration, Destination::Personas, false),
            Ok(TourMove::Show(Some(TourPlace::Stage(Stage::Narration))))
        );
        assert_eq!(
            app.tour_next(),
            TourMove::Show(Some(TourPlace::Stage(Stage::Narration)))
        );
    }

    #[test]
    fn a_missing_scene_passes_over_its_step() {
        // No scene picked, so no inspector: the scene step goes. A picked
        // scene with nothing to draw again lights the inspector instead,
        // and with no inspector either the card centres.
        let mut run = TourRun::new(&SCENES, 2, Destination::Projects);
        assert_eq!(
            run.spot(|anchor| anchor != TourAnchor::Inspector),
            Spot::Skip
        );
        run.next();
        assert_eq!(
            run.spot(|anchor| anchor == TourAnchor::Inspector),
            Spot::Lit(TourAnchor::Inspector)
        );
        assert_eq!(run.spot(|_| false), Spot::Center);
    }

    #[test]
    fn a_screen_with_nothing_to_show_offers_no_tour() {
        let app = start(&FakeProgress::default());
        assert_eq!(app.screen_tour(Destination::Research, false), None);
        assert_eq!(app.screen_tour(Destination::Costs, true), None, "no tour");
    }

    #[test]
    fn the_first_visit_with_content_marks_the_tour_new() {
        let app = start(&FakeProgress::default());
        assert_eq!(
            app.screen_tour(Destination::Themes, true),
            Some(ScreenTour {
                tour: TourId::Themes,
                new: true
            })
        );
        assert_eq!(app.tour_step(false), None, "it never starts on its own");
    }

    #[test]
    fn completing_or_dismissing_the_tour_takes_the_mark_away() {
        let progress = FakeProgress::default();
        let mut app = start(&progress);
        app.start_tour(TourId::Research, Destination::Research, false)
            .unwrap();
        assert_eq!(mark(&app, Destination::Research), Some(true), "under way");
        app.tour_close();
        assert_eq!(
            mark(&app, Destination::Research),
            Some(true),
            "closed midway"
        );
        app.resume_tour(Destination::Research, false).unwrap();
        while app.tour_next() != TourMove::Finished {}
        assert_eq!(mark(&app, Destination::Research), Some(false), "completed");

        app.start_tour(TourId::Themes, Destination::Themes, false)
            .unwrap();
        assert_eq!(app.tour_skip(), TourMove::Left(Destination::Themes));
        assert_eq!(mark(&app, Destination::Themes), Some(false), "dismissed");

        let app = start(&progress);
        assert_eq!(mark(&app, Destination::Research), Some(false), "kept");
        assert_eq!(mark(&app, Destination::Performance), Some(true));
    }

    #[test]
    fn new_content_brings_the_mark_back() {
        let progress = FakeProgress::default();
        // Completed at a version below the tour's (the tours are at 1, and
        // the fake keeps a 0 the database would refuse).
        saved(
            &progress,
            TourId::Performance,
            PERFORMANCE.version - 1,
            TourState::Completed,
        );
        let app = start(&progress);
        assert_eq!(mark(&app, Destination::Performance), Some(true));
    }

    #[test]
    fn turning_the_setting_off_hides_the_mark_and_keeps_the_tour() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let secrets = Arc::new(bardo_storage::MemorySecretStore::default());
        let mut app = Bardo::start(
            Repositories::shared(Arc::clone(&db), secrets.clone()),
            testing::providers(),
            None,
        )
        .unwrap();
        assert!(app.offers_screen_tours(), "on by default");
        app.set_offer_screen_tours(false).unwrap();
        assert_eq!(mark(&app, Destination::Research), Some(false));

        let mut app = Bardo::start(
            Repositories::shared(db, secrets),
            testing::providers(),
            None,
        )
        .unwrap();
        assert!(!app.offers_screen_tours(), "remembered");
        app.set_offer_screen_tours(true).unwrap();
        assert_eq!(mark(&app, Destination::Research), Some(true));
    }

    #[test]
    fn reset_tours_marks_screen_tours_new_again() {
        let progress = FakeProgress::default();
        saved(
            &progress,
            TourId::Themes,
            THEMES.version,
            TourState::Dismissed,
        );
        let mut app = start(&progress);
        assert_eq!(mark(&app, Destination::Themes), Some(false));
        app.reset_tours().unwrap();
        assert_eq!(mark(&app, Destination::Themes), Some(true));
    }

    #[test]
    fn the_missed_posts_list_runs_its_own_tour_and_holds_the_others() {
        let mut app = start(&FakeProgress::default());
        assert_eq!(
            app.start_tour(TourId::Publish, Destination::Projects, true),
            Err(TourError::MissedPostsOpen),
            "the others wait behind the list"
        );
        assert_eq!(
            app.start_tour(TourId::Missed, Destination::Projects, true),
            Ok(TourMove::Show(Some(TourPlace::Missed)))
        );
        assert_eq!(
            app.tour_step(true).map(|step| step.tour),
            Some(TourId::Missed)
        );
        app.tour_next();
        assert_eq!(at(&app), 2);
        assert_eq!(
            app.tour_step(false).map(|step| step.number),
            Some(2),
            "and without it"
        );
    }

    #[test]
    fn the_missed_posts_tour_offers_itself_while_the_list_is_up() {
        let mut app = start(&FakeProgress::default());
        assert_eq!(app.place_tour(TourPlace::Missed, false), None, "no list");
        assert_eq!(
            app.place_tour(TourPlace::Missed, true),
            Some(ScreenTour {
                tour: TourId::Missed,
                new: true
            })
        );
        app.start_tour(TourId::Missed, Destination::Projects, true)
            .unwrap();
        while app.tour_next() != TourMove::Finished {}
        assert_eq!(
            app.place_tour(TourPlace::Missed, true).map(|tour| tour.new),
            Some(false)
        );
    }

    #[test]
    fn a_missed_post_step_without_its_buttons_lights_the_list() {
        // The first post is being given a new time: its buttons are gone.
        let run = TourRun::new(&MISSED, 1, Destination::Projects);
        assert_eq!(
            run.spot(|anchor| anchor == control(Control::MissedList)),
            Spot::Lit(control(Control::MissedList))
        );
        assert_eq!(run.spot(|_| false), Spot::Center, "the list is closed");
    }

    #[test]
    fn the_networks_tab_offers_its_tour_and_opens_the_tab() {
        let mut app = start(&FakeProgress::default());
        let networks = TourPlace::Settings(SettingsTab::Networks);
        assert_eq!(
            app.place_tour(networks, true).map(|tour| tour.tour),
            Some(TourId::Networks)
        );
        assert_eq!(
            app.place_tour(TourPlace::Settings(SettingsTab::Appearance), true),
            None
        );
        assert_eq!(
            app.start_tour(TourId::Networks, Destination::Projects, false),
            Ok(TourMove::Show(Some(networks)))
        );
    }

    #[test]
    fn the_publish_tour_falls_back_where_a_network_does_not_upload() {
        let mut run = TourRun::new(&PUBLISH, 2, Destination::Projects);
        let upload = control(Control::PublishUpload);
        // No realistic voice: the upload, where the review asks.
        assert_eq!(run.spot(|anchor| anchor == upload), Spot::Lit(upload));
        // X or Kick: nothing to upload, so the card says it in the middle.
        run.next();
        assert_eq!(run.spot(|_| false), Spot::Center);
        // The review is closed: its "When" row gives way to the upload.
        run.next();
        assert_eq!(run.spot(|anchor| anchor == upload), Spot::Lit(upload));
    }

    #[test]
    fn moves_without_a_tour_do_nothing() {
        let mut app = start(&FakeProgress::default());
        assert_eq!(app.tour_next(), TourMove::Closed);
        assert_eq!(app.tour_back(), TourMove::Closed);
        assert_eq!(app.tour_skip(), TourMove::Closed);
        assert_eq!(app.tour_step_over(), TourMove::Closed);
        assert_eq!(app.tour_spot(|_| true), None);
    }

    #[test]
    fn works_against_real_sqlite() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let secrets = Arc::new(bardo_storage::MemorySecretStore::default());
        let mut app = Bardo::start(
            Repositories::shared(Arc::clone(&db), secrets.clone()),
            testing::providers(),
            None,
        )
        .unwrap();
        app.start_tour(TourId::Welcome, Destination::Projects, false)
            .unwrap();
        app.tour_next();
        app.tour_close();
        let mut app = Bardo::start(
            Repositories::shared(db, secrets),
            testing::providers(),
            None,
        )
        .unwrap();
        assert!(!app.tour_offer());
        app.resume_tour(Destination::Projects, false).unwrap();
        assert_eq!(at(&app), 2);
    }
}
