//! Products screen: the products to advertise as a collection, and the
//! brief, the images and the logo of the picked one as its inspector.
//! Pictures are imported through the system's file dialog once the user
//! confirms they may use them; nothing on this screen is paid. Rules and
//! storage live in `bardo_app`; this file maps the form to a
//! `ProductDraft` and results back to the screen.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use bardo_app::bardo_domain::{
    ImageFormat, LibraryItem, LibraryKind, MAX_PRODUCT_IMAGES, Product, ProductBrief, ProductDraft,
    ProductFieldError, ProductId, ProductImage, ProductImageError, ProductImageId,
    ProductImageKind, ProductKind, ProductLogo, ProductOwnership, ProductPicture, ProjectId,
    Sharing, split_facts,
};
use bardo_app::{Bardo, Control, Destination, LibraryUse, ProductError, Text, TourAnchor};
use gpui_kit::component::button::{Button, ButtonGroup, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Selectable as _, Sizable as _, StyledExt as _, h_flex,
    v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Entity, EventEmitter, Image, ObjectFit, PathPromptOptions,
    ScrollHandle, SharedString, Subscription, Task, Window, div, img, px,
};

use crate::ad_angles::{AnglesSection, OpenProject};
use crate::kit::{self, Tone};
use crate::library::{self, BringForm, LibraryScope};
use crate::parts::{Collection, CollectionKind, Header, Inspector, ScreenParts, Tile};
use crate::product_concept::ConceptSection;
use crate::product_sheet::SheetSection;
use crate::sharing::{self, ImportPanel, SharingEvent, SharingPanel};
use crate::shell::tr;
use crate::{guide, layout};

/// Field errors each control shows, and clears once the user edits it.
const NAME_ERRORS: &[ProductFieldError] = &[
    ProductFieldError::NameRequired,
    ProductFieldError::NameTooLong,
];
const DESCRIPTION_ERRORS: &[ProductFieldError] = &[
    ProductFieldError::DescriptionRequired,
    ProductFieldError::DescriptionTooLong,
];
const AUDIENCE_ERRORS: &[ProductFieldError] = &[ProductFieldError::AudienceTooLong];
const FACTS_ERRORS: &[ProductFieldError] = &[
    ProductFieldError::FactTooLong,
    ProductFieldError::TooManyFacts,
];
const OFFER_ERRORS: &[ProductFieldError] = &[ProductFieldError::OfferTooLong];
const LINK_ERRORS: &[ProductFieldError] = &[
    ProductFieldError::LinkInvalid,
    ProductFieldError::LinkTooLong,
];
const CALL_TO_ACTION_ERRORS: &[ProductFieldError] = &[ProductFieldError::CallToActionTooLong];
const MUST_SAY_ERRORS: &[ProductFieldError] = &[ProductFieldError::MustSayTooLong];
const NEVER_SAY_ERRORS: &[ProductFieldError] = &[ProductFieldError::NeverSayTooLong];

/// The side of an image's thumbnail.
const THUMBNAIL: f32 = 132.;

/// What the form footer says after the last action.
enum Notice {
    Saved,
    Duplicated(String),
    Archived,
    Restored,
    Deleted(String),
    /// Brought into the open project: the copy's name and the project's.
    Brought(String, String),
    Error(Text),
}

/// What the pictures section says after the last action.
#[derive(Clone, Copy)]
enum PictureNotice {
    ImageAdded,
    ImageRemoved,
    LogoAdded,
    LogoRemoved,
    /// The action failed, and, for a refused picture, why.
    Error(Text, Option<ProductImageError>),
}

/// Which picture the rights card is about to import.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Importing {
    Image,
    Logo,
}

/// The picture the user is asked whether to remove.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Removing {
    Image(ProductImageId),
    Logo,
}

/// A picture ready to show: decoded once.
fn decoded(picture: &ProductPicture) -> Arc<Image> {
    let format = match picture.format() {
        ImageFormat::Png => gpui_kit::ImageFormat::Png,
        ImageFormat::Jpeg => gpui_kit::ImageFormat::Jpeg,
        ImageFormat::Webp => gpui_kit::ImageFormat::Webp,
    };
    Arc::new(Image::from_bytes(format, picture.bytes().to_vec()))
}

/// What the form shows of the product.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Brief,
    Sheet,
    Angles,
}

impl Tab {
    const ALL: [Tab; 3] = [Tab::Brief, Tab::Sheet, Tab::Angles];

    fn label(self) -> Text {
        match self {
            Tab::Brief => Text::ProductTabBrief,
            Tab::Sheet => Text::ProductTabSheet,
            Tab::Angles => Text::ProductTabAngles,
        }
    }
}

struct ShownImage {
    image: ProductImage,
    picture: Arc<Image>,
}

struct ShownLogo {
    logo: ProductLogo,
    picture: Arc<Image>,
}

impl EventEmitter<OpenProject> for ProductsScreen {}

pub struct ProductsScreen {
    bardo: Entity<Bardo>,
    /// The open project's products, or the global library's.
    scope: LibraryScope,
    /// Whose products the list shows and new ones go to (`None`: the
    /// global library), read when the screen reloads.
    owner: Option<ProjectId>,
    /// A project scope with no project open.
    no_project: bool,
    /// *Bring into the project* in progress for the edited product.
    bring: Option<BringForm>,
    /// The uses of the edited global product, to confirm before the save
    /// goes through.
    confirm: Option<Vec<LibraryUse>>,
    products: Vec<Product>,
    load_failed: bool,
    show_archived: bool,
    /// Scrolls the form, so a tour can bring its parts into view.
    form_scroll: ScrollHandle,
    /// The product being edited; `None` while creating a new one.
    editing: Option<ProductId>,
    tab: Tab,
    /// The Sheet tab, which reads and edits the product's sheet itself.
    sheet: Entity<SheetSection>,
    /// The Sheet tab is not showing the edited product as it is stored
    /// (or not yet): the next frame that shows the tab reads it.
    sheet_pending: bool,
    /// The Ad angles tab, which reads and reviews the product's angles
    /// itself.
    angles: Entity<AnglesSection>,
    /// As `sheet_pending`, for the Ad angles tab.
    angles_pending: bool,
    /// The concept image of an idea (#280), under the images; it reads
    /// with them.
    concept: Entity<ConceptSection>,
    name: Entity<InputState>,
    kind: ProductKind,
    description: Entity<TextareaState>,
    audience: Entity<TextareaState>,
    facts: Entity<TextareaState>,
    offer: Entity<InputState>,
    link: Entity<InputState>,
    call_to_action: Entity<InputState>,
    ownership: ProductOwnership,
    must_say: Entity<TextareaState>,
    never_say: Entity<TextareaState>,
    /// The saved brief the form was filled from; `None` for a new product.
    filled: Option<ProductBrief>,
    field_errors: Vec<ProductFieldError>,
    notice: Option<Notice>,
    /// Asking whether to delete the edited product.
    confirm_delete: bool,
    images: Vec<ShownImage>,
    logo: Option<ShownLogo>,
    pictures_failed: bool,
    /// The pictures on show are not the edited product's (or not yet
    /// read): the next frame reads them, so they are read only while the
    /// screen shows.
    pictures_pending: bool,
    /// Asking whether to remove a picture.
    confirm_remove: Option<Removing>,
    /// The rights card, and whether its box is ticked.
    importing: Option<(Importing, bool)>,
    /// A file dialog that is open.
    dialog: Option<Task<()>>,
    picture_notice: Option<PictureNotice>,
    /// The edited product's sharing, below its actions.
    sharing: Entity<SharingPanel>,
    /// *Import from another project* and *Open item file…*.
    import: Entity<ImportPanel>,
    /// How the listed products take part in sharing, for their tags.
    shared: HashMap<LibraryItem, Sharing>,
    _subscriptions: Vec<Subscription>,
}

impl ProductsScreen {
    pub fn new(
        bardo: Entity<Bardo>,
        scope: LibraryScope,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx));
        let description = cx.new(|cx| TextareaState::new(window, cx).auto_grow(3, 10));
        let audience = cx.new(|cx| TextareaState::new(window, cx).auto_grow(2, 8));
        let facts = cx.new(|cx| TextareaState::new(window, cx).auto_grow(4, 14));
        let offer = cx.new(|cx| InputState::new(window, cx));
        let link = cx.new(|cx| InputState::new(window, cx));
        let call_to_action = cx.new(|cx| InputState::new(window, cx));
        let must_say = cx.new(|cx| TextareaState::new(window, cx).auto_grow(2, 6));
        let never_say = cx.new(|cx| TextareaState::new(window, cx).auto_grow(2, 6));

        let subscriptions = vec![
            cx.subscribe(&name, |this, _, event, cx| {
                this.edited(NAME_ERRORS, event, cx)
            }),
            cx.subscribe(&description, |this, _, event, cx| {
                this.edited(DESCRIPTION_ERRORS, event, cx)
            }),
            cx.subscribe(&audience, |this, _, event, cx| {
                this.edited(AUDIENCE_ERRORS, event, cx)
            }),
            cx.subscribe(&facts, |this, _, event, cx| {
                this.edited(FACTS_ERRORS, event, cx)
            }),
            cx.subscribe(&offer, |this, _, event, cx| {
                this.edited(OFFER_ERRORS, event, cx)
            }),
            cx.subscribe(&link, |this, _, event, cx| {
                this.edited(LINK_ERRORS, event, cx)
            }),
            cx.subscribe(&call_to_action, |this, _, event, cx| {
                this.edited(CALL_TO_ACTION_ERRORS, event, cx)
            }),
            cx.subscribe(&must_say, |this, _, event, cx| {
                this.edited(MUST_SAY_ERRORS, event, cx)
            }),
            cx.subscribe(&never_say, |this, _, event, cx| {
                this.edited(NEVER_SAY_ERRORS, event, cx)
            }),
            cx.observe_in(&bardo, window, |this, _, window, cx| {
                this.relabel(window, cx)
            }),
        ];

        let form_scroll = ScrollHandle::new();
        let sheet = cx.new(|cx| SheetSection::new(bardo.clone(), form_scroll.clone(), window, cx));
        let concept = cx.new(|cx| ConceptSection::new(bardo.clone(), cx));
        let angles =
            cx.new(|cx| AnglesSection::new(bardo.clone(), form_scroll.clone(), window, cx));
        let mut subscriptions = subscriptions;
        let sharing = cx.new(|_| SharingPanel::new(bardo.clone(), "product-sharing"));
        let import =
            cx.new(|_| ImportPanel::new(bardo.clone(), "product-import", LibraryKind::Product));
        subscriptions.push(
            cx.subscribe_in(&sharing, window, |this, _, event, window, cx| {
                this.shared_changed(*event, window, cx)
            }),
        );
        subscriptions.push(
            cx.subscribe_in(&import, window, |this, _, event, window, cx| {
                this.shared_changed(*event, window, cx)
            }),
        );
        // An angle's ad project opens on the Projects screen.
        subscriptions.push(cx.subscribe(&angles, |_, _, event: &OpenProject, cx| {
            cx.emit(OpenProject(event.0))
        }));

        let mut screen = Self {
            bardo,
            scope,
            owner: None,
            no_project: false,
            bring: None,
            confirm: None,
            products: Vec::new(),
            load_failed: false,
            show_archived: false,
            form_scroll,
            editing: None,
            tab: Tab::Brief,
            sheet,
            sheet_pending: true,
            angles,
            angles_pending: true,
            concept,
            name,
            kind: ProductKind::default(),
            description,
            audience,
            facts,
            offer,
            link,
            call_to_action,
            ownership: ProductOwnership::default(),
            must_say,
            never_say,
            filled: None,
            field_errors: Vec::new(),
            notice: None,
            confirm_delete: false,
            images: Vec::new(),
            logo: None,
            pictures_failed: false,
            pictures_pending: false,
            confirm_remove: None,
            importing: None,
            dialog: None,
            picture_notice: None,
            sharing,
            import,
            shared: HashMap::new(),
            _subscriptions: subscriptions,
        };
        screen.reload_list(cx);
        screen.relabel(window, cx);
        // Open on the first product, so the screen shows a filled form.
        if let Some(first) = screen.first_listed() {
            screen.edit(first, window, cx);
        }
        screen
    }

    /// Re-reads the products: a restored backup or an imported settings
    /// file may have changed them.
    pub fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let before = self.owner;
        self.reload_list(cx);
        self.sheet_pending = true;
        self.angles_pending = true;
        if self.owner != before {
            // Another project opened: its products show.
            match self.first_listed() {
                Some(first) => self.edit(first, window, cx),
                None => self.start_new(window, cx),
            }
        } else if let Some(id) = self.editing {
            let stored = self
                .products
                .iter()
                .find(|p| p.id == id)
                .map(|p| p.brief.clone());
            match stored {
                // Changed elsewhere: show it, unless the form holds edits
                // of the user's own.
                Some(stored) if self.filled.as_ref() != Some(&stored) && !self.dirty(cx) => {
                    self.edit(id, window, cx)
                }
                Some(_) => self.pictures_pending = true,
                None => self.start_new(window, cx),
            }
        }
        cx.notify();
    }

    /// Whether the form differs from the saved brief it was filled from.
    fn dirty(&self, cx: &App) -> bool {
        match (&self.filled, ProductBrief::validate(self.draft(cx))) {
            (Some(filled), Ok(now)) => *filled != now,
            _ => true,
        }
    }

    /// A sharing change: the list re-reads, and shows an arrived item.
    fn shared_changed(&mut self, event: SharingEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            SharingEvent::Changed => self.reload(window, cx),
            SharingEvent::Show(LibraryItem::Product(id)) => {
                self.reload_list(cx);
                self.edit(id, window, cx);
            }
            SharingEvent::Show(_) => self.reload(window, cx),
        }
    }

    fn reload_list(&mut self, cx: &mut Context<Self>) {
        let (owner, no_project) = self.scope.owner(self.bardo.read(cx));
        self.owner = owner;
        self.no_project = no_project;
        let project_scope = self.scope == LibraryScope::Project;
        self.import
            .update(cx, |import, cx| import.set_owner(owner, project_scope, cx));
        self.shared = sharing::sharing_map(self.bardo.read(cx), owner, LibraryKind::Product);
        self.sharing.update(cx, |sharing, cx| sharing.refresh(cx));
        if no_project {
            self.products.clear();
            self.load_failed = false;
            return;
        }
        match self.bardo.read(cx).library_products(owner) {
            Ok(products) => {
                self.products = products;
                self.load_failed = false;
            }
            Err(_) => self.load_failed = true,
        }
    }

    /// The products the list shows: archived ones only when asked for,
    /// and always the one being edited.
    fn listed(&self) -> impl Iterator<Item = &Product> {
        self.products
            .iter()
            .filter(|p| self.show_archived || !p.archived || self.editing == Some(p.id))
    }

    fn first_listed(&self) -> Option<ProductId> {
        self.listed().next().map(|p| p.id)
    }

    fn archived_count(&self) -> usize {
        self.products.iter().filter(|p| p.archived).count()
    }

    /// Reads the edited product's pictures. Pictures already on show
    /// keep their decoding; the ones that left free theirs.
    fn load_pictures(&mut self, cx: &mut Context<Self>) {
        self.pictures_pending = false;
        let editing = self.editing;
        self.concept
            .update(cx, |concept, cx| concept.show(editing, cx));
        let mut kept: Vec<ShownImage> = std::mem::take(&mut self.images);
        let old_logo = self.logo.take();
        let read = self.editing.map(|id| {
            let bardo = self.bardo.read(cx);
            (bardo.product_images(id), bardo.product_logo(id))
        });
        self.pictures_failed = false;
        match read {
            Some((Ok(images), Ok(logo))) => {
                self.images = images
                    .into_iter()
                    .map(
                        |image| match kept.iter().position(|s| s.image.id == image.id) {
                            Some(ix) => {
                                let mut shown = kept.swap_remove(ix);
                                shown.image = image;
                                shown
                            }
                            None => ShownImage {
                                picture: decoded(&image.picture),
                                image,
                            },
                        },
                    )
                    .collect();
                let mut old_logo = old_logo;
                self.logo = logo.map(|logo| match old_logo.take() {
                    Some(shown) if shown.logo == logo => shown,
                    other => {
                        old_logo = other;
                        ShownLogo {
                            picture: decoded(&logo.picture),
                            logo,
                        }
                    }
                });
                if let Some(shown) = old_logo {
                    shown.picture.remove_asset(cx);
                }
            }
            Some(_) => {
                self.pictures_failed = true;
                if let Some(shown) = old_logo {
                    shown.picture.remove_asset(cx);
                }
            }
            None => {
                if let Some(shown) = old_logo {
                    shown.picture.remove_asset(cx);
                }
            }
        }
        for shown in kept {
            shown.picture.remove_asset(cx);
        }
    }

    /// Any edit hides the "saved" notice, so it never describes unsaved
    /// edits, and drops a pending deletion.
    fn touched(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.notice, Some(Notice::Error(_)) | None) {
            self.notice = None;
        }
        self.confirm_delete = false;
        // A save's uses or a copy's name were asked about other text.
        self.confirm = None;
        self.bring = None;
        cx.notify();
    }

    fn edited(&mut self, fields: &[ProductFieldError], event: &InputEvent, cx: &mut Context<Self>) {
        if !matches!(event, InputEvent::Change) {
            return;
        }
        self.field_errors.retain(|error| !fields.contains(error));
        self.touched(cx);
    }

    /// Placeholders follow the interface language.
    fn relabel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let bardo = self.bardo.read(cx);
        let inputs = [
            (&self.name, Text::ProductNamePlaceholder),
            (&self.offer, Text::ProductOfferPlaceholder),
            (&self.link, Text::ProductLinkPlaceholder),
            (&self.call_to_action, Text::ProductCallToActionPlaceholder),
        ]
        .map(|(input, text)| (input.clone(), tr(bardo, text)));
        let areas = [
            (&self.description, Text::ProductDescriptionPlaceholder),
            (&self.audience, Text::ProductAudiencePlaceholder),
            (&self.facts, Text::ProductFactsPlaceholder),
            (&self.must_say, Text::ProductMustSayPlaceholder),
            (&self.never_say, Text::ProductNeverSayPlaceholder),
        ]
        .map(|(area, text)| (area.clone(), tr(bardo, text)));
        for (input, text) in inputs {
            input.update(cx, |input, cx| input.set_placeholder(text, window, cx));
        }
        for (area, text) in areas {
            area.update(cx, |area, cx| area.set_placeholder(text, window, cx));
        }
        cx.notify();
    }

    fn fill(&mut self, draft: &ProductDraft, window: &mut Window, cx: &mut Context<Self>) {
        let inputs = [
            (&self.name, &draft.name),
            (&self.offer, &draft.offer),
            (&self.link, &draft.link),
            (&self.call_to_action, &draft.call_to_action),
        ];
        for (input, value) in inputs {
            let value = value.clone();
            input.update(cx, |input, cx| input.set_value(value, window, cx));
        }
        let areas = [
            (&self.description, &draft.description),
            (&self.audience, &draft.audience),
            (&self.facts, &draft.facts),
            (&self.must_say, &draft.must_say),
            (&self.never_say, &draft.never_say),
        ];
        for (area, value) in areas {
            let value = value.clone();
            area.update(cx, |area, cx| area.set_value(value, window, cx));
        }
        self.kind = draft.kind;
        self.ownership = draft.ownership;
    }

    fn draft(&self, cx: &App) -> ProductDraft {
        ProductDraft {
            name: self.name.read(cx).value().to_string(),
            kind: self.kind,
            description: self.description.read(cx).value().to_string(),
            audience: self.audience.read(cx).value().to_string(),
            facts: self.facts.read(cx).value().to_string(),
            offer: self.offer.read(cx).value().to_string(),
            link: self.link.read(cx).value().to_string(),
            call_to_action: self.call_to_action.read(cx).value().to_string(),
            ownership: self.ownership,
            must_say: self.must_say.read(cx).value().to_string(),
            never_say: self.never_say.read(cx).value().to_string(),
        }
    }

    fn reset_feedback(&mut self) {
        self.field_errors.clear();
        self.notice = None;
        self.bring = None;
        self.confirm = None;
        self.confirm_delete = false;
        self.importing = None;
        self.confirm_remove = None;
        self.picture_notice = None;
        // A file dialog still open was for the product on show before:
        // dropping it stops waiting for its answer.
        self.dialog = None;
    }

    fn start_new(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = None;
        // A product has a sheet only once it is saved.
        self.tab = Tab::Brief;
        self.sheet_pending = true;
        self.angles_pending = true;
        self.filled = None;
        self.reset_feedback();
        self.fill(&ProductDraft::default(), window, cx);
        self.pictures_pending = true;
        self.sharing
            .update(cx, |sharing, cx| sharing.show(None, cx));
        cx.notify();
    }

    fn edit(&mut self, id: ProductId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(product) = self.products.iter().find(|p| p.id == id) else {
            return;
        };
        let draft = ProductDraft::from(&product.brief);
        let shown = (LibraryItem::Product(id), product.brief.name().to_owned());
        self.sharing
            .update(cx, |sharing, cx| sharing.show(Some(shown), cx));
        self.filled = Some(product.brief.clone());
        self.editing = Some(id);
        self.sheet_pending = true;
        self.angles_pending = true;
        self.reset_feedback();
        self.fill(&draft, window, cx);
        self.pictures_pending = true;
        cx.notify();
    }

    /// Saves the form; `false` when it could not be, with the reason on
    /// show.
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.save_confirmed(false, window, cx)
    }

    /// Saves the form; `confirmed` says the user saw where the edited
    /// global product is used. `false` when it could not be saved, with
    /// the reason or the uses on show.
    fn save_confirmed(
        &mut self,
        confirmed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.no_project && self.editing.is_none() {
            return false;
        }
        let draft = self.draft(cx);
        let bardo = self.bardo.read(cx);
        let result = match self.editing {
            Some(id) => bardo.update_product(id, draft, confirmed),
            None => bardo.create_product_in(self.owner, draft),
        };
        let saved = match result {
            Err(ProductError::UsedIn(uses)) => {
                self.field_errors.clear();
                self.notice = None;
                self.confirm = Some(uses);
                false
            }
            Ok(saved) => {
                self.confirm = None;
                self.reload_list(cx);
                let created = self.editing.is_none();
                self.editing = Some(saved.id);
                // Facts added to the brief are on the sheet now.
                self.sheet_pending = true;
                self.angles_pending = true;
                // The pictures section stays as it is: saving the brief
                // says nothing about a picture being chosen.
                self.field_errors.clear();
                self.confirm_delete = false;
                // Show the normalized values (trimmed text, the link's
                // scheme, the facts one per line).
                self.fill(&ProductDraft::from(&saved.brief), window, cx);
                self.filled = Some(saved.brief);
                if created {
                    self.pictures_pending = true;
                }
                // A kind or a description saved changes whether a concept
                // image is offered and what drawing one costs.
                let editing = self.editing;
                self.concept
                    .update(cx, |concept, cx| concept.show(editing, cx));
                self.notice = Some(Notice::Saved);
                true
            }
            Err(error) => {
                self.show_error(&error);
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
        // The copy is of what the form shows: edits not yet saved are
        // saved first, and a form that cannot be saved is not copied.
        if self.dirty(cx) && !self.save(window, cx) {
            return;
        }
        // On a project's entry a global product's copy is the project's
        // own, so it shows in this list.
        match self.bardo.read(cx).duplicate_product_into(id, self.owner) {
            Ok(copy) => {
                self.reload_list(cx);
                self.edit(copy.id, window, cx);
                self.notice = Some(Notice::Duplicated(copy.brief.name().to_owned()));
            }
            Err(error) => self.show_error(&error),
        }
        cx.notify();
    }

    fn set_archived(&mut self, archived: bool, cx: &mut Context<Self>) {
        let Some(id) = self.editing else {
            return;
        };
        match self.bardo.read(cx).set_product_archived(id, archived) {
            Ok(_) => {
                self.reload_list(cx);
                // An archived product takes no new angles.
                self.angles_pending = true;
                self.confirm_delete = false;
                self.notice = Some(if archived {
                    Notice::Archived
                } else {
                    Notice::Restored
                });
            }
            Err(error) => self.show_error(&error),
        }
        cx.notify();
    }

    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.editing else {
            return;
        };
        let name = self
            .products
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.brief.name().to_owned())
            .unwrap_or_default();
        match self.bardo.read(cx).delete_product(id) {
            Ok(()) => {
                self.reload_list(cx);
                match self.first_listed() {
                    Some(next) => self.edit(next, window, cx),
                    None => self.start_new(window, cx),
                }
                self.notice = Some(Notice::Deleted(name));
            }
            Err(error) => self.show_error(&error),
        }
        cx.notify();
    }

    /// Opens *Bring into the project* for the edited global product.
    fn start_bring(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(product) = self.edited_product() else {
            return;
        };
        let name = product.brief.name().to_owned();
        self.confirm = None;
        self.notice = None;
        self.bring = Some(BringForm::new(&name, window, cx));
        cx.notify();
    }

    /// Copies the edited global product into the open project under the
    /// typed name; a taken name comes back with a suggestion in the field.
    fn bring_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(id), Some(form)) = (self.editing, &self.bring) else {
            return;
        };
        let Some((project, project_name)) = library::open_project(self.bardo.read(cx)) else {
            return;
        };
        let name = form.typed(cx);
        // Edits not yet saved go to the copy, never to the global one.
        let draft = self.dirty(cx).then(|| self.draft(cx));
        let bardo = self.bardo.read(cx);
        let result = bardo
            .bring_product_into_project(id, project, Some(&name))
            .and_then(|copy| match draft {
                Some(draft) => bardo.update_product(
                    copy.id,
                    ProductDraft {
                        name: copy.brief.name().to_owned(),
                        ..draft
                    },
                    true,
                ),
                None => Ok(copy),
            });
        match result {
            Ok(copy) => {
                self.reload_list(cx);
                let shown = self.products.iter().any(|product| product.id == copy.id);
                // Off the project's list, the form goes back to the global
                // product as saved: its edits went to the copy.
                self.edit(if shown { copy.id } else { id }, window, cx);
                self.bring = None;
                self.notice = Some(Notice::Brought(copy.brief.name().to_owned(), project_name));
            }
            Err(ProductError::NameClash { suggestion }) => {
                if let Some(form) = &mut self.bring {
                    form.error = Some(Text::LibraryNameClash);
                    form.name
                        .update(cx, |input, cx| input.set_value(suggestion, window, cx));
                }
            }
            Err(error) => {
                if let Some(form) = &mut self.bring {
                    form.error = error.form_message().or(Some(Text::ProductNotSaved));
                }
            }
        }
        cx.notify();
    }

    /// The product being edited, as listed.
    fn edited_product(&self) -> Option<&Product> {
        let id = self.editing?;
        self.products.iter().find(|product| product.id == id)
    }

    /// The uses of the edited global product before its save, or *Bring
    /// into the project* (its button, or its form once open).
    fn render_library(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut parts = Vec::new();
        let bardo = self.bardo.read(cx);
        if let Some(uses) = &self.confirm {
            let title = SharedString::from(
                bardo.text_with(Text::LibraryUsesTitle, &[("n", &uses.len().to_string())]),
            );
            let lines = uses.iter().map(library::use_line).collect();
            parts.push(library::confirm_uses(
                "product-save",
                &self.bardo,
                (title, lines),
                |this: &mut Self, window, cx| {
                    this.save_confirmed(true, window, cx);
                },
                |this: &mut Self, _, cx| {
                    this.confirm = None;
                    cx.notify();
                },
                cx,
            ));
        }
        let bardo = self.bardo.read(cx);
        let global = self.edited_product().filter(|product| product.is_global());
        if let (Some(_), Some((_, project))) = (global, library::open_project(bardo)) {
            parts.push(match &self.bring {
                Some(form) => library::render_bring(
                    "product-bring",
                    &self.bardo,
                    (form, &project),
                    |this: &mut Self, window, cx| this.bring_in(window, cx),
                    |this: &mut Self, _, cx| {
                        this.bring = None;
                        cx.notify();
                    },
                    cx,
                ),
                None => library::bring_button(
                    "bring-product",
                    &self.bardo,
                    |this: &mut Self, window, cx| this.start_bring(window, cx),
                    cx,
                ),
            });
        }
        parts
    }

    fn show_error(&mut self, error: &ProductError) {
        self.field_errors = error.field_errors().to_vec();
        self.notice = error.form_message().map(Notice::Error);
        self.confirm_delete = false;
    }

    fn picture_failed(&mut self, error: &ProductError) {
        self.picture_notice = error
            .form_message()
            .map(|text| PictureNotice::Error(text, error.image_reason()));
    }

    /// Asks for the picture the rights card is about, then imports it.
    fn choose_picture(&mut self, cx: &mut Context<Self>) {
        let (Some(product), Some((target, confirmed))) = (self.editing, self.importing) else {
            return;
        };
        let prompt = match target {
            Importing::Image => Text::ProductImageDialog,
            Importing::Logo => Text::ProductLogoDialog,
        };
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(tr(self.bardo.read(cx), prompt)),
        });
        self.picture_notice = None;
        self.dialog = Some(cx.spawn(async move |this, cx| {
            let chosen = chosen.await;
            let _ = this.update(cx, |this, cx| {
                this.dialog = None;
                // Only for the product and the ticked card it was opened
                // from: the user may have unticked or closed it meanwhile.
                if this.editing != Some(product) || this.importing != Some((target, true)) {
                    cx.notify();
                    return;
                }
                match chosen {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.first() {
                            let bardo = this.bardo.read(cx);
                            let result = match target {
                                Importing::Image => bardo
                                    .import_product_image(product, path, confirmed)
                                    .map(|_| PictureNotice::ImageAdded),
                                Importing::Logo => bardo
                                    .import_product_logo(product, path, confirmed)
                                    .map(|_| PictureNotice::LogoAdded),
                            };
                            match result {
                                Ok(notice) => {
                                    this.importing = None;
                                    this.picture_notice = Some(notice);
                                }
                                Err(error) => this.picture_failed(&error),
                            }
                            this.pictures_pending = true;
                        }
                    }
                    Ok(Ok(None)) | Err(_) => {}
                    Ok(Err(_)) => {
                        this.picture_notice =
                            Some(PictureNotice::Error(Text::FileDialogFailed, None))
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn set_image_kind(
        &mut self,
        image: ProductImageId,
        kind: ProductImageKind,
        cx: &mut Context<Self>,
    ) {
        let Some(product) = self.editing else {
            return;
        };
        match self
            .bardo
            .read(cx)
            .set_product_image_kind(product, image, kind)
        {
            Ok(()) => {
                self.picture_notice = None;
                if let Some(shown) = self.images.iter_mut().find(|s| s.image.id == image) {
                    shown.image.kind = kind;
                }
            }
            Err(error) => {
                self.picture_failed(&error);
                self.pictures_pending = true;
            }
        }
        cx.notify();
    }

    fn remove_image(&mut self, image: ProductImageId, cx: &mut Context<Self>) {
        let Some(product) = self.editing else {
            return;
        };
        self.confirm_remove = None;
        match self.bardo.read(cx).remove_product_image(product, image) {
            Ok(()) => self.picture_notice = Some(PictureNotice::ImageRemoved),
            Err(error) => self.picture_failed(&error),
        }
        self.pictures_pending = true;
        cx.notify();
    }

    fn remove_logo(&mut self, cx: &mut Context<Self>) {
        let Some(product) = self.editing else {
            return;
        };
        self.confirm_remove = None;
        match self.bardo.read(cx).remove_product_logo(product) {
            Ok(()) => self.picture_notice = Some(PictureNotice::LogoRemoved),
            Err(error) => self.picture_failed(&error),
        }
        self.pictures_pending = true;
        cx.notify();
    }

    /// Whether there are products to show: the screen offers its tour
    /// only then.
    pub fn has_content(&self) -> bool {
        !self.load_failed && !self.products.is_empty()
    }

    fn collection(&self, cx: &mut Context<Self>) -> Collection {
        let mut collection = Collection::new(CollectionKind::List, "product-list");
        let bardo = self.bardo.read(cx);
        let tiles: Vec<Tile> = self
            .listed()
            .enumerate()
            .map(|(ix, product)| {
                let id = product.id;
                let mut tile = Tile::new(
                    ("product", ix),
                    Rc::new(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.edit(id, window, cx)
                    })),
                );
                tile.selected = self.editing == Some(id);
                tile.title = Some(SharedString::from(product.brief.name().to_owned()));
                let kind = tr(bardo, Text::ProductKindName(product.brief.kind()));
                // Shown even when archived takes the status: editing a
                // global product changes it everywhere.
                tile.text = Some(
                    if self.scope == LibraryScope::Project && product.is_global() {
                        SharedString::from(format!(
                            "{kind} · {}",
                            tr(bardo, Text::LibraryGlobalTag)
                        ))
                    } else {
                        kind
                    },
                );
                tile.status = if product.archived {
                    Some(
                        kit::status(Tone::Neutral, tr(bardo, Text::ProductArchivedTag), cx)
                            .into_any_element(),
                    )
                } else {
                    (self.scope == LibraryScope::Project && product.is_global())
                        .then(|| library::global_tag(bardo, cx))
                        .or_else(|| {
                            let shared = self.shared.get(&LibraryItem::Product(product.id))?;
                            sharing::sharing_tag(bardo, shared, cx)
                        })
                };
                tile
            })
            .collect();
        collection.empty = Some(if self.load_failed {
            kit::notice(Tone::Danger, tr(bardo, Text::ProductsNotLoaded), cx).into_any_element()
        } else {
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(tr(
                    bardo,
                    if self.no_project {
                        Text::LibraryNoProject
                    } else {
                        Text::ProductsEmpty
                    },
                ))
                .into_any_element()
        });
        if !self.load_failed {
            collection.tiles = tiles;
        }
        collection
    }

    fn error_for(&self, fields: &[ProductFieldError], cx: &App) -> Option<AnyElement> {
        let error = self.field_errors.iter().find(|e| fields.contains(e))?;
        Some(
            div()
                .text_xs()
                .text_color(cx.theme().danger)
                .child(tr(self.bardo.read(cx), Text::ProductFieldError(*error)))
                .into_any_element(),
        )
    }

    fn render_kind(&self, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let group = ButtonGroup::new("product-kind")
            .small()
            .outline()
            .children(ProductKind::ALL.map(|kind| {
                Button::new(("product-kind", kind as usize))
                    .label(tr(bardo, Text::ProductKindName(kind)))
                    .selected(kind == self.kind)
            }))
            .on_click(cx.listener(|this, clicked: &Vec<usize>, _, cx| {
                if let Some(kind) = clicked.first().and_then(|&i| ProductKind::ALL.get(i)) {
                    this.kind = *kind;
                    this.touched(cx);
                }
            }));
        kit::field(
            tr(bardo, Text::ProductKind),
            Some(guide::info(
                bardo,
                "product-kind-info",
                tr(bardo, Text::ProductKindHint),
                guide::refs::PRODUCTS_BRIEF,
            )),
            h_flex().flex_wrap().child(group).into_any_element(),
            None,
        )
        .into_any_element()
    }

    fn render_ownership(&self, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let group = ButtonGroup::new("product-ownership")
            .small()
            .outline()
            .children(ProductOwnership::ALL.map(|owner| {
                Button::new(("product-ownership", owner as usize))
                    .label(tr(bardo, Text::ProductOwnershipName(owner)))
                    .selected(owner == self.ownership)
            }))
            .on_click(cx.listener(|this, clicked: &Vec<usize>, _, cx| {
                if let Some(owner) = clicked.first().and_then(|&i| ProductOwnership::ALL.get(i)) {
                    this.ownership = *owner;
                    this.touched(cx);
                }
            }));
        kit::field(
            tr(bardo, Text::ProductOwnership),
            Some(guide::info(
                bardo,
                "product-ownership-info",
                tr(bardo, Text::ProductOwnershipHint),
                guide::refs::PRODUCTS_OWNER,
            )),
            h_flex().flex_wrap().child(group).into_any_element(),
            None,
        )
        .into_any_element()
    }

    fn render_facts(&self, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let read = split_facts(&self.facts.read(cx).value()).len();
        let below = self.error_for(FACTS_ERRORS, cx).unwrap_or_else(|| {
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(SharedString::from(bardo.text_with(
                    Text::ProductFactsCount,
                    &[("n", &read.to_string())],
                )))
                .into_any_element()
        });
        kit::field(
            tr(bardo, Text::ProductFacts),
            Some(guide::info(
                bardo,
                "product-facts-info",
                tr(bardo, Text::ProductFactsHint),
                guide::refs::PRODUCTS_FACTS,
            )),
            Textarea::new(&self.facts).into_any_element(),
            Some(below),
        )
        .into_any_element()
    }

    /// The card that asks the user to confirm they may use the picture
    /// before choosing its file.
    fn render_rights(
        &self,
        target: Importing,
        confirmed: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let choose = match target {
            Importing::Image => Text::ProductChooseImage,
            Importing::Logo => Text::ProductChooseLogo,
        };
        kit::card(cx)
            .p_3()
            .gap_2()
            .border_color(theme.warning)
            .child(
                div()
                    .font_semibold()
                    .child(tr(bardo, Text::ProductRightsTitle)),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(tr(bardo, Text::ProductRightsHint)),
            )
            .child(
                Checkbox::new("product-rights-confirm")
                    .min_w_0()
                    .checked(confirmed)
                    .label(tr(bardo, Text::ProductRightsConfirm))
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        if let Some((_, confirmed)) = this.importing.as_mut() {
                            *confirmed = *checked;
                        }
                        cx.notify();
                    })),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        Button::new("choose-product-picture")
                            .small()
                            .primary()
                            .label(tr(bardo, choose))
                            .disabled(!confirmed || self.dialog.is_some())
                            .on_click(
                                cx.listener(|this, _: &ClickEvent, _, cx| this.choose_picture(cx)),
                            ),
                    )
                    .child(
                        Button::new("cancel-product-picture")
                            .small()
                            .ghost()
                            .label(tr(bardo, Text::ProductCancel))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.importing = None;
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    fn thumbnail(picture: &Arc<Image>, cx: &App) -> AnyElement {
        div()
            .flex_none()
            .size(px(THUMBNAIL))
            .rounded_md()
            .overflow_hidden()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().muted)
            .child(
                img(picture.clone())
                    .size_full()
                    .object_fit(ObjectFit::Contain),
            )
            .into_any_element()
    }

    /// Asks before a picture and its confirmation go: that cannot be
    /// undone.
    fn render_remove_ask(
        &self,
        id: (&'static str, usize),
        ask: Text,
        remove: Text,
        on_remove: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bardo = self.bardo.read(cx);
        v_flex()
            .gap_1()
            .child(div().text_xs().child(tr(bardo, ask)))
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_1()
                    .child(
                        Button::new((SharedString::from(format!("confirm-remove-{}", id.0)), id.1))
                            .xsmall()
                            .danger()
                            .label(tr(bardo, remove))
                            .on_click(on_remove),
                    )
                    .child(
                        Button::new((SharedString::from(format!("cancel-remove-{}", id.0)), id.1))
                            .xsmall()
                            .ghost()
                            .label(tr(bardo, Text::ProductCancel))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.confirm_remove = None;
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    fn render_image(&self, ix: usize, shown: &ShownImage, cx: &mut Context<Self>) -> AnyElement {
        let image = &shown.image;
        let id = image.id;
        let remove = if self.confirm_remove == Some(Removing::Image(id)) {
            let on_remove =
                cx.listener(move |this, _: &ClickEvent, _, cx| this.remove_image(id, cx));
            self.render_remove_ask(
                ("product-image", ix),
                Text::ProductImageRemoveAsk,
                Text::RemoveProductImage,
                on_remove,
                cx,
            )
        } else {
            let label = tr(self.bardo.read(cx), Text::RemoveProductImage);
            h_flex()
                .child(
                    Button::new(("remove-product-image", ix))
                        .xsmall()
                        .outline()
                        .label(label)
                        .disabled(self.sharing.read(cx).is_linked())
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.confirm_remove = Some(Removing::Image(id));
                            cx.notify();
                        })),
                )
                .into_any_element()
        };
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let kinds = ButtonGroup::new(("product-image-kind", ix))
            .xsmall()
            .outline()
            .disabled(self.sharing.read(cx).is_linked())
            .children(ProductImageKind::ALL.map(|kind| {
                Button::new(("product-image-kind", ix * 10 + kind as usize))
                    .label(tr(bardo, Text::ProductImageKindName(kind)))
                    .selected(kind == image.kind)
            }))
            .on_click(cx.listener(move |this, clicked: &Vec<usize>, _, cx| {
                if let Some(kind) = clicked.first().and_then(|&i| ProductImageKind::ALL.get(i)) {
                    this.set_image_kind(id, *kind, cx);
                }
            }));
        let size = bardo.text_with(
            Text::ProductImageSize,
            &[
                ("width", &image.picture.width().to_string()),
                ("height", &image.picture.height().to_string()),
            ],
        );
        let confirmed = bardo.text_with(
            Text::ProductRightsConfirmedAt,
            &[("when", &bardo.product_time_text(image.confirmed_at))],
        );
        v_flex()
            .w(px(THUMBNAIL + 24.))
            .gap_1()
            .child(Self::thumbnail(&shown.picture, cx))
            .child(h_flex().child(kinds))
            .when(image.cut_out(), |col| {
                col.child(h_flex().child(kit::status(
                    Tone::Accent,
                    tr(bardo, Text::ProductImageCutOut),
                    cx,
                )))
            })
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(SharedString::from(size)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(SharedString::from(confirmed)),
            )
            .child(remove)
            .into_any_element()
    }

    fn render_picture_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let (tone, text, reason) = match self.picture_notice? {
            PictureNotice::ImageAdded => (Tone::Success, Text::ProductImageAdded, None),
            PictureNotice::ImageRemoved => (Tone::Neutral, Text::ProductImageRemoved, None),
            PictureNotice::LogoAdded => (Tone::Success, Text::ProductLogoAdded, None),
            PictureNotice::LogoRemoved => (Tone::Neutral, Text::ProductLogoRemoved, None),
            PictureNotice::Error(text, reason) => (Tone::Danger, text, reason),
        };
        let reason = reason.map(|reason| {
            let (width, height) = match reason {
                ProductImageError::TooSmall { width, height }
                | ProductImageError::TooLarge { width, height }
                | ProductImageError::Shape { width, height } => (width, height),
                ProductImageError::NotAnImage
                | ProductImageError::LogoNotPng
                | ProductImageError::FileTooLarge(_) => (0, 0),
            };
            let megabytes = match reason {
                ProductImageError::FileTooLarge(bytes) => bytes.div_ceil(1_000_000),
                _ => 0,
            };
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(SharedString::from(bardo.text_with(
                    Text::ProductImageReason(reason),
                    &[
                        ("width", &width.to_string()),
                        ("height", &height.to_string()),
                        ("mb", &megabytes.to_string()),
                    ],
                )))
        });
        Some(
            v_flex()
                .gap_0p5()
                .child(kit::notice(tone, tr(bardo, text), cx))
                .children(reason)
                .into_any_element(),
        )
    }

    fn render_images(&self, cx: &mut Context<Self>) -> AnyElement {
        let tiles: Vec<AnyElement> = self
            .images
            .iter()
            .enumerate()
            .map(|(ix, shown)| self.render_image(ix, shown, cx))
            .collect();
        let rights = match self.importing {
            Some((Importing::Image, confirmed)) => {
                Some(self.render_rights(Importing::Image, confirmed, cx))
            }
            _ => None,
        };
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let full = self.images.len() >= MAX_PRODUCT_IMAGES;
        let count = bardo.text_with(
            Text::ProductImageCount,
            &[("n", &self.images.len().to_string())],
        );
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .text_sm()
                            .font_medium()
                            .child(SharedString::from(count)),
                    )
                    .child(guide::info(
                        bardo,
                        "product-image-kind-info",
                        tr(bardo, Text::ProductImageKindHint),
                        guide::refs::PRODUCTS_PICTURES,
                    )),
            )
            .map(|col| {
                if tiles.is_empty() {
                    col.child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(tr(bardo, Text::ProductImagesNone)),
                    )
                } else {
                    col.child(h_flex().flex_wrap().items_start().gap_4().children(tiles))
                }
            })
            .map(|col| match rights {
                Some(rights) => col.child(rights),
                None => col.child(
                    h_flex().child(
                        Button::new("add-product-image")
                            .small()
                            .outline()
                            .label(tr(bardo, Text::AddProductImage))
                            .disabled(
                                full || self.dialog.is_some() || self.sharing.read(cx).is_linked(),
                            )
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.importing = Some((Importing::Image, false));
                                this.picture_notice = None;
                                cx.notify();
                            })),
                    ),
                ),
            })
            .into_any_element()
    }

    fn render_logo(&self, cx: &mut Context<Self>) -> AnyElement {
        let rights = match self.importing {
            Some((Importing::Logo, confirmed)) => {
                Some(self.render_rights(Importing::Logo, confirmed, cx))
            }
            _ => None,
        };
        let remove_ask = (self.confirm_remove == Some(Removing::Logo)).then(|| {
            self.render_remove_ask(
                ("product-logo", 0),
                Text::ProductLogoRemoveAsk,
                Text::RemoveProductLogo,
                cx.listener(|this, _: &ClickEvent, _, cx| this.remove_logo(cx)),
                cx,
            )
        });
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let current = match &self.logo {
            Some(shown) => {
                let confirmed = bardo.text_with(
                    Text::ProductRightsConfirmedAt,
                    &[("when", &bardo.product_time_text(shown.logo.confirmed_at))],
                );
                let size = bardo.text_with(
                    Text::ProductImageSize,
                    &[
                        ("width", &shown.logo.picture.width().to_string()),
                        ("height", &shown.logo.picture.height().to_string()),
                    ],
                );
                h_flex()
                    .gap_3()
                    .items_start()
                    .child(Self::thumbnail(&shown.picture, cx))
                    .child(
                        v_flex()
                            .gap_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(SharedString::from(size)),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(SharedString::from(confirmed)),
                            ),
                    )
                    .into_any_element()
            }
            None => div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(tr(bardo, Text::ProductLogoNone))
                .into_any_element(),
        };
        let add = if self.logo.is_some() {
            Text::ReplaceProductLogo
        } else {
            Text::AddProductLogo
        };
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .font_medium()
                            .child(tr(bardo, Text::ProductLogoTitle)),
                    )
                    .child(guide::info(
                        bardo,
                        "product-logo-info",
                        tr(bardo, Text::ProductLogoHint),
                        guide::refs::PRODUCTS_LOGO,
                    )),
            )
            .child(current)
            .children(remove_ask)
            .map(|col| match rights {
                Some(rights) => col.child(rights),
                None => col.child(
                    h_flex()
                        .flex_wrap()
                        .gap_2()
                        .child(
                            Button::new("add-product-logo")
                                .small()
                                .outline()
                                .label(tr(bardo, add))
                                .disabled(
                                    self.dialog.is_some() || self.sharing.read(cx).is_linked(),
                                )
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.importing = Some((Importing::Logo, false));
                                    this.picture_notice = None;
                                    cx.notify();
                                })),
                        )
                        .when(self.logo.is_some(), |row| {
                            row.child(
                                Button::new("remove-product-logo")
                                    .small()
                                    .ghost()
                                    .label(tr(bardo, Text::RemoveProductLogo))
                                    .disabled(
                                        self.confirm_remove == Some(Removing::Logo)
                                            || self.sharing.read(cx).is_linked(),
                                    )
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.confirm_remove = Some(Removing::Logo);
                                        cx.notify();
                                    })),
                            )
                        }),
                ),
            })
            .into_any_element()
    }

    /// The images and the logo; a product not yet saved has none.
    fn render_pictures(&self, cx: &mut Context<Self>) -> AnyElement {
        let body: AnyElement = if self.editing.is_none() {
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(tr(self.bardo.read(cx), Text::ProductPicturesSaveFirst))
                .into_any_element()
        } else if self.pictures_failed {
            kit::notice(
                Tone::Danger,
                tr(self.bardo.read(cx), Text::ProductPicturesNotLoaded),
                cx,
            )
            .into_any_element()
        } else {
            let images = self.render_images(cx);
            let logo = self.render_logo(cx);
            let notice = self.render_picture_notice(cx);
            let scroll = Some(&self.form_scroll);
            v_flex()
                .gap_4()
                .child(kit::anchor_in(
                    TourAnchor::Control(Control::ProductPictures),
                    images,
                    scroll,
                ))
                .child(kit::anchor_in(
                    TourAnchor::Control(Control::ProductLogo),
                    logo,
                    scroll,
                ))
                .children(notice)
                .into_any_element()
        };
        let bardo = self.bardo.read(cx);
        kit::card(cx)
            .p_4()
            .gap_3()
            .child(
                v_flex()
                    .gap_1()
                    .child(kit::section_heading(tr(bardo, Text::ProductPicturesTitle)))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(tr(bardo, Text::ProductPicturesHint)),
                    ),
            )
            .child(body)
            .into_any_element()
    }

    fn render_delete(&self, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let name = self
            .editing
            .and_then(|id| self.products.iter().find(|p| p.id == id))
            .map(|p| p.brief.name().to_owned())
            .unwrap_or_default();
        kit::card(cx)
            .p_4()
            .gap_2()
            .border_color(theme.danger)
            .child(div().font_semibold().child(SharedString::from(
                bardo.text_with(Text::ProductDeleteTitle, &[("name", &name)]),
            )))
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(tr(bardo, Text::ProductDeleteHint)),
            )
            .child(
                h_flex()
                    .gap_2()
                    .pt_1()
                    .child(
                        Button::new("confirm-delete-product")
                            .danger()
                            .label(tr(bardo, Text::ConfirmDeleteProduct))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.delete(window, cx)
                            })),
                    )
                    .child(
                        Button::new("cancel-delete-product")
                            .ghost()
                            .label(tr(bardo, Text::ProductCancel))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.confirm_delete = false;
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    /// The switch between the brief, the sheet and the ad angles.
    fn render_tabs(&self, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let group = ButtonGroup::new("product-tab")
            .small()
            .outline()
            .children(Tab::ALL.map(|tab| {
                Button::new(("product-tab", tab as usize))
                    .label(tr(bardo, tab.label()))
                    .selected(tab == self.tab)
            }))
            .on_click(cx.listener(|this, clicked: &Vec<usize>, _, cx| {
                if let Some(tab) = clicked.first().and_then(|&i| Tab::ALL.get(i)) {
                    this.tab = *tab;
                    // Claims confirmed on the Sheet tab since are what an
                    // angle may lean on.
                    this.angles_pending = true;
                    cx.notify();
                }
            }));
        kit::anchor_in(
            TourAnchor::Control(Control::ProductSheet),
            h_flex().child(group),
            Some(&self.form_scroll),
        )
        .into_any_element()
    }

    fn render_form(&self, cx: &mut Context<Self>) -> AnyElement {
        let tabs = self.render_tabs(cx);
        if self.tab != Tab::Brief {
            // A run works from the saved brief, so edits waiting in the
            // other tab are pointed out.
            let unsaved = (self.editing.is_some() && self.dirty(cx)).then(|| {
                kit::notice(
                    Tone::Warning,
                    tr(
                        self.bardo.read(cx),
                        if self.tab == Tab::Sheet {
                            Text::SheetBriefUnsaved
                        } else {
                            Text::AdAnglesBriefUnsaved
                        },
                    ),
                    cx,
                )
            });
            let column = v_flex()
                .max_w(px(720.))
                .gap_4()
                .child(tabs)
                .children(unsaved);
            return div()
                .child(if self.tab == Tab::Sheet {
                    column.child(self.sheet.clone())
                } else {
                    column.child(self.angles.clone())
                })
                .into_any_element();
        }
        let kind = self.render_kind(cx);
        let ownership = self.render_ownership(cx);
        let facts = self.render_facts(cx);
        let pictures = self.render_pictures(cx);
        let delete = self.confirm_delete.then(|| self.render_delete(cx));
        let library_parts = self.render_library(cx);
        let linked = self.sharing.read(cx).is_linked();
        let has_sharing = self.sharing.read(cx).has_content();
        let global_note = self
            .edited_product()
            .filter(|product| self.scope == LibraryScope::Project && product.is_global())
            .map(|_| library::global_note(self.bardo.read(cx), cx));
        let errors = [
            NAME_ERRORS,
            DESCRIPTION_ERRORS,
            AUDIENCE_ERRORS,
            OFFER_ERRORS,
            LINK_ERRORS,
            CALL_TO_ACTION_ERRORS,
            MUST_SAY_ERRORS,
            NEVER_SAY_ERRORS,
        ]
        .map(|fields| self.error_for(fields, cx));
        let [
            name_error,
            description_error,
            audience_error,
            offer_error,
            link_error,
            call_to_action_error,
            must_say_error,
            never_say_error,
        ] = errors;
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let scroll = Some(&self.form_scroll);
        let field = |label: Text, control: AnyElement, below: Option<AnyElement>| {
            kit::field(tr(bardo, label), None, control, below)
        };
        let archived = self
            .editing
            .and_then(|id| self.products.iter().find(|p| p.id == id))
            .is_some_and(|p| p.archived);

        let (title, action) = match self.editing {
            Some(_) => (Text::EditProductTitle, Text::SaveProduct),
            None => (Text::NewProductTitle, Text::CreateProduct),
        };
        // Shrinks and wraps beside the buttons; a copy's name can be long.
        let notice = self.notice.as_ref().map(|notice| {
            let (tone, text): (Tone, SharedString) = match notice {
                Notice::Saved => (Tone::Success, tr(bardo, Text::ProductSaved)),
                Notice::Duplicated(name) => (
                    Tone::Success,
                    bardo
                        .text_with(Text::ProductDuplicated, &[("name", name)])
                        .into(),
                ),
                Notice::Archived => (Tone::Neutral, tr(bardo, Text::ProductArchived)),
                Notice::Restored => (Tone::Success, tr(bardo, Text::ProductRestored)),
                Notice::Deleted(name) => (
                    Tone::Neutral,
                    bardo
                        .text_with(Text::ProductDeleted, &[("name", name)])
                        .into(),
                ),
                Notice::Brought(name, project) => (
                    Tone::Success,
                    bardo
                        .text_with(
                            Text::LibraryBrought,
                            &[("name", name), ("project", project)],
                        )
                        .into(),
                ),
                Notice::Error(text) => (Tone::Danger, tr(bardo, *text)),
            };
            kit::notice(tone, text, cx)
        });

        let actions = h_flex()
            .flex_wrap()
            .gap_3()
            .child(
                Button::new("save-product")
                    .primary()
                    .label(tr(bardo, action))
                    .disabled(
                        self.confirm.is_some()
                            || linked
                            || (self.no_project && self.editing.is_none()),
                    )
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.save(window, cx);
                    })),
            )
            .when(self.editing.is_some(), |row| {
                row.child(
                    Button::new("duplicate-product")
                        .outline()
                        .label(tr(bardo, Text::DuplicateProduct))
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.duplicate(window, cx)
                        })),
                )
                .child(
                    Button::new("archive-product")
                        .outline()
                        .label(tr(
                            bardo,
                            if archived {
                                Text::RestoreProduct
                            } else {
                                Text::ArchiveProduct
                            },
                        ))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.set_archived(!archived, cx)
                        })),
                )
                .child(
                    Button::new("delete-product")
                        .ghost()
                        .label(tr(bardo, Text::DeleteProduct))
                        .disabled(self.confirm_delete)
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.confirm_delete = true;
                            this.notice = None;
                            cx.notify();
                        })),
                )
                .child(guide::info(
                    bardo,
                    "product-archive-info",
                    tr(bardo, Text::ProductActionsHint),
                    guide::refs::PRODUCTS_ARCHIVE,
                ))
            })
            .children(notice.map(|notice| notice.flex_1().min_w_0()));

        div()
            .child(
                v_flex()
                    .max_w(px(720.))
                    .gap_4()
                    .child(tabs)
                    .child(
                        v_flex()
                            .gap_1()
                            .child(kit::title(tr(bardo, title)))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme.muted_foreground)
                                    .child(tr(bardo, Text::ProductBriefHint)),
                            )
                            .children(global_note),
                    )
                    .child(self.import.clone())
                    .children(library_parts)
                    .child(
                        kit::card(cx)
                            .p_4()
                            .gap_4()
                            .child(kit::anchor_in(
                                TourAnchor::Control(Control::ProductBrief),
                                v_flex()
                                    .gap_4()
                                    .child(field(
                                        Text::ProductName,
                                        Input::new(&self.name).into_any_element(),
                                        name_error,
                                    ))
                                    .child(kind)
                                    .child(field(
                                        Text::ProductDescription,
                                        Textarea::new(&self.description).into_any_element(),
                                        description_error,
                                    ))
                                    .child(field(
                                        Text::ProductAudience,
                                        Textarea::new(&self.audience).into_any_element(),
                                        audience_error,
                                    ))
                                    .child(field(
                                        Text::ProductOffer,
                                        Input::new(&self.offer).into_any_element(),
                                        offer_error,
                                    ))
                                    .child(field(
                                        Text::ProductLink,
                                        Input::new(&self.link).into_any_element(),
                                        link_error,
                                    ))
                                    .child(field(
                                        Text::ProductCallToAction,
                                        Input::new(&self.call_to_action).into_any_element(),
                                        call_to_action_error,
                                    )),
                                scroll,
                            ))
                            .child(kit::anchor_in(
                                TourAnchor::Control(Control::ProductFacts),
                                facts,
                                scroll,
                            ))
                            .child(kit::anchor_in(
                                TourAnchor::Control(Control::ProductOwnership),
                                v_flex()
                                    .gap_4()
                                    .child(ownership)
                                    .child(field(
                                        Text::ProductMustSay,
                                        Textarea::new(&self.must_say).into_any_element(),
                                        must_say_error,
                                    ))
                                    .child(field(
                                        Text::ProductNeverSay,
                                        Textarea::new(&self.never_say).into_any_element(),
                                        never_say_error,
                                    )),
                                scroll,
                            )),
                    )
                    .children(delete)
                    .child(kit::anchor_in(
                        TourAnchor::Control(Control::ProductActions),
                        actions,
                        scroll,
                    ))
                    .when(has_sharing, |form| {
                        form.child(kit::anchor_in(
                            TourAnchor::Control(Control::LibrarySharing),
                            self.sharing.clone(),
                            scroll,
                        ))
                    })
                    .child(pictures)
                    .child(self.concept.clone()),
            )
            .into_any_element()
    }
}

impl Render for ProductsScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.pictures_pending {
            self.load_pictures(cx);
        }
        // A tour's step about a part of the form shows the tab it is on.
        let toured = match self
            .bardo
            .read(cx)
            .tour_step(false)
            .and_then(|step| step.anchor)
        {
            Some(TourAnchor::Control(Control::ProductResearch | Control::ProductClaims)) => {
                Some(Tab::Sheet)
            }
            Some(TourAnchor::Control(
                Control::ProductAngles | Control::ProductAngleReview | Control::ProductAngleActions,
            )) => Some(Tab::Angles),
            Some(TourAnchor::Control(
                Control::ProductBrief
                | Control::ProductFacts
                | Control::ProductOwnership
                | Control::ProductActions
                | Control::ProductPictures
                | Control::ProductLogo,
            )) => Some(Tab::Brief),
            _ => None,
        };
        if let Some(tab) = toured {
            if self.tab != tab {
                // Shown as stored, like a tab the user picks.
                self.angles_pending = true;
            }
            self.tab = tab;
        }
        if self.tab == Tab::Sheet && self.sheet_pending {
            self.sheet_pending = false;
            let product = self.editing;
            self.sheet
                .update(cx, |sheet, cx| sheet.show(product, window, cx));
        }
        if self.tab == Tab::Angles && self.angles_pending {
            self.angles_pending = false;
            let product = self.editing;
            self.angles
                .update(cx, |angles, cx| angles.show(product, cx));
        }
        let collection = self.collection(cx);
        let form = self.render_form(cx);
        let archived = self.archived_count();
        let bardo = self.bardo.read(cx);
        let mut header = Header::place(bardo, Destination::Products, cx);
        let info = guide::info(
            bardo,
            "products-info",
            tr(bardo, Text::ProductsHint),
            guide::refs::PRODUCTS_LIBRARY,
        )
        .into_any_element();
        header.info = guide::header_info(
            bardo,
            Destination::Products,
            self.has_content(),
            Some(info),
            cx,
        );
        let mut actions = Vec::new();
        if archived > 0 {
            actions.push(
                Checkbox::new("show-archived-products")
                    .small()
                    .checked(self.show_archived)
                    .label(SharedString::from(bardo.text_with(
                        Text::ProductsShowArchived,
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
            Button::new("new-product")
                .small()
                .primary()
                .label(tr(bardo, Text::NewProduct))
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
