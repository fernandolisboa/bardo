//! Products (docs/spec/marketing-videos.md, CONTEXT.md): things to
//! advertise, kept in a library owned by the user profile like personas
//! and reusable across channels and videos. A product is its brief (what
//! the user tells Bardo about it), up to three images and an optional
//! logo. Nothing here calls a model.

use std::sync::Arc;
use std::time::SystemTime;

use crate::persona::numbered_copy;
use crate::{
    ImageFormat, ProductSheet, ProfileId, RepositoryError, image_declares_transparency,
    image_dimensions,
};

uuid_id!(
    /// Identifies a product.
    ProductId
);

uuid_id!(
    /// Identifies one image of a product.
    ProductImageId
);

/// What kind of thing a product is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ProductKind {
    /// An app, a site, a course, a file: something used on a screen.
    #[default]
    Digital,
    /// Something that can be held or photographed.
    Physical,
    /// Work done for the customer.
    Service,
    /// Only an idea: it does not exist yet, and its videos never say it
    /// does.
    Idea,
}

impl ProductKind {
    pub const ALL: [ProductKind; 4] = [
        ProductKind::Digital,
        ProductKind::Physical,
        ProductKind::Service,
        ProductKind::Idea,
    ];

    /// Stable name stored in the database and the settings file.
    pub fn code(self) -> &'static str {
        match self {
            ProductKind::Digital => "digital",
            ProductKind::Physical => "physical",
            ProductKind::Service => "service",
            ProductKind::Idea => "idea",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.code() == code)
    }
}

/// Whose a product is. It sets the default commercial disclosure of the
/// product's videos and nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ProductOwnership {
    /// The user's own product or business.
    #[default]
    Mine,
    /// Someone else's, promoted for them.
    Client,
}

impl ProductOwnership {
    pub const ALL: [ProductOwnership; 2] = [ProductOwnership::Mine, ProductOwnership::Client];

    /// Stable name stored in the database and the settings file.
    pub fn code(self) -> &'static str {
        match self {
            ProductOwnership::Mine => "mine",
            ProductOwnership::Client => "client",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|owner| owner.code() == code)
    }
}

/// A product brief as the user typed it, before validation.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProductDraft {
    pub name: String,
    pub kind: ProductKind,
    /// What it is.
    pub description: String,
    /// Who it is for.
    pub audience: String,
    /// The facts the user stands behind, one per line.
    pub facts: String,
    /// Price, discount, deadline.
    pub offer: String,
    pub link: String,
    pub call_to_action: String,
    pub ownership: ProductOwnership,
    /// What an ad must say.
    pub must_say: String,
    /// What an ad must never say.
    pub never_say: String,
}

/// Why a draft is not a valid brief. One entry per problem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProductFieldError {
    NameRequired,
    NameTooLong,
    DescriptionRequired,
    DescriptionTooLong,
    AudienceTooLong,
    /// One of the facts is over the limit.
    FactTooLong,
    TooManyFacts,
    OfferTooLong,
    /// Not a web address: another scheme, spaces, a user name, no site.
    LinkInvalid,
    LinkTooLong,
    CallToActionTooLong,
    MustSayTooLong,
    NeverSayTooLong,
}

impl ProductFieldError {
    /// In the order the form shows its fields.
    pub const ALL: [ProductFieldError; 13] = [
        ProductFieldError::NameRequired,
        ProductFieldError::NameTooLong,
        ProductFieldError::DescriptionRequired,
        ProductFieldError::DescriptionTooLong,
        ProductFieldError::AudienceTooLong,
        ProductFieldError::OfferTooLong,
        ProductFieldError::LinkInvalid,
        ProductFieldError::LinkTooLong,
        ProductFieldError::CallToActionTooLong,
        ProductFieldError::FactTooLong,
        ProductFieldError::TooManyFacts,
        ProductFieldError::MustSayTooLong,
        ProductFieldError::NeverSayTooLong,
    ];
}

/// What the user tells Bardo about a product, always valid: a name and a
/// description, text within limits, the facts one per entry and a link
/// that is a web address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductBrief {
    name: String,
    kind: ProductKind,
    description: String,
    audience: String,
    facts: Vec<String>,
    offer: String,
    link: String,
    call_to_action: String,
    ownership: ProductOwnership,
    must_say: String,
    never_say: String,
}

impl ProductBrief {
    /// Limits are in characters, not bytes.
    pub const MAX_NAME_CHARS: usize = 80;
    pub const MAX_DESCRIPTION_CHARS: usize = 2_000;
    pub const MAX_AUDIENCE_CHARS: usize = 1_000;
    pub const MAX_FACT_CHARS: usize = 300;
    pub const MAX_FACTS: usize = 30;
    pub const MAX_OFFER_CHARS: usize = 300;
    pub const MAX_LINK_CHARS: usize = 500;
    pub const MAX_CALL_TO_ACTION_CHARS: usize = 120;
    pub const MAX_NOTE_CHARS: usize = 1_000;

    /// Validates and normalizes a draft: surrounding whitespace is
    /// trimmed, the facts are split one per line and a link typed without
    /// its scheme gets `https://`.
    pub fn validate(draft: ProductDraft) -> Result<Self, Vec<ProductFieldError>> {
        let mut errors = Vec::new();
        let mut text = |raw: &str, max: usize, error: ProductFieldError| {
            let text = raw.trim().to_owned();
            if too_long(&text, max) {
                errors.push(error);
            }
            text
        };

        // One line each: a line break or another control character typed
        // or brought in by a file reads as a space.
        let line = |raw: &str| -> String {
            raw.chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .collect()
        };
        let name = text(
            &line(&draft.name),
            Self::MAX_NAME_CHARS,
            ProductFieldError::NameTooLong,
        );
        let description = text(
            &draft.description,
            Self::MAX_DESCRIPTION_CHARS,
            ProductFieldError::DescriptionTooLong,
        );
        let audience = text(
            &draft.audience,
            Self::MAX_AUDIENCE_CHARS,
            ProductFieldError::AudienceTooLong,
        );
        let offer = text(
            &line(&draft.offer),
            Self::MAX_OFFER_CHARS,
            ProductFieldError::OfferTooLong,
        );
        let call_to_action = text(
            &line(&draft.call_to_action),
            Self::MAX_CALL_TO_ACTION_CHARS,
            ProductFieldError::CallToActionTooLong,
        );
        let must_say = text(
            &draft.must_say,
            Self::MAX_NOTE_CHARS,
            ProductFieldError::MustSayTooLong,
        );
        let never_say = text(
            &draft.never_say,
            Self::MAX_NOTE_CHARS,
            ProductFieldError::NeverSayTooLong,
        );
        if name.is_empty() {
            errors.push(ProductFieldError::NameRequired);
        }
        if description.is_empty() {
            errors.push(ProductFieldError::DescriptionRequired);
        }

        let facts = split_facts(&draft.facts);
        if facts
            .iter()
            .any(|fact| too_long(fact, Self::MAX_FACT_CHARS))
        {
            errors.push(ProductFieldError::FactTooLong);
        }
        if facts.len() > Self::MAX_FACTS {
            errors.push(ProductFieldError::TooManyFacts);
        }

        let link = match normalize_link(&draft.link) {
            Some(link) if too_long(&link, Self::MAX_LINK_CHARS) => {
                errors.push(ProductFieldError::LinkTooLong);
                link
            }
            Some(link) => link,
            None => {
                errors.push(ProductFieldError::LinkInvalid);
                String::new()
            }
        };

        if !errors.is_empty() {
            // In the order the form shows its fields.
            errors.sort_by_key(|error| {
                ProductFieldError::ALL
                    .iter()
                    .position(|other| other == error)
            });
            return Err(errors);
        }
        Ok(Self {
            name,
            kind: draft.kind,
            description,
            audience,
            facts,
            offer,
            link,
            call_to_action,
            ownership: draft.ownership,
            must_say,
            never_say,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn kind(&self) -> ProductKind {
        self.kind
    }

    /// What it is.
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Who it is for.
    pub fn audience(&self) -> &str {
        &self.audience
    }

    /// The facts the user stands behind, one statement each, in the
    /// user's own words.
    pub fn facts(&self) -> &[String] {
        &self.facts
    }

    pub fn offer(&self) -> &str {
        &self.offer
    }

    /// The product's web address with its scheme, or empty.
    pub fn link(&self) -> &str {
        &self.link
    }

    pub fn call_to_action(&self) -> &str {
        &self.call_to_action
    }

    pub fn ownership(&self) -> ProductOwnership {
        self.ownership
    }

    pub fn must_say(&self) -> &str {
        &self.must_say
    }

    pub fn never_say(&self) -> &str {
        &self.never_say
    }

    /// Whether two product names would read as the same to the user.
    pub fn same_name(&self, other: &str) -> bool {
        self.name.to_lowercase() == other.trim().to_lowercase()
    }

    /// A name for a copy of this product that none of `taken` uses, as
    /// for a persona's copy.
    pub fn copy_name(&self, suffix: &str, taken: impl Fn(&str) -> bool) -> String {
        numbered_copy(&self.name, Self::MAX_NAME_CHARS, suffix, taken)
    }
}

impl From<&ProductBrief> for ProductDraft {
    fn from(brief: &ProductBrief) -> Self {
        Self {
            name: brief.name.clone(),
            kind: brief.kind,
            description: brief.description.clone(),
            audience: brief.audience.clone(),
            facts: brief.facts.join("\n"),
            offer: brief.offer.clone(),
            link: brief.link.clone(),
            call_to_action: brief.call_to_action.clone(),
            ownership: brief.ownership,
            must_say: brief.must_say.clone(),
            never_say: brief.never_say.clone(),
        }
    }
}

fn too_long(text: &str, max_chars: usize) -> bool {
    text.chars().count() > max_chars
}

/// The facts of a brief: one per line, trimmed, blank lines dropped. The
/// list marks the user typed or pasted before a fact (`-`, `*`, `•`, `·`,
/// `▪`, `–`, `—`, each followed by a space or a tab) are not part of the
/// statement. Every leading mark goes, so a fact read back splits the
/// same way.
pub fn split_facts(text: &str) -> Vec<String> {
    const MARKS: [char; 7] = ['-', '*', '•', '·', '▪', '–', '—'];
    text.lines()
        .map(|line| {
            let mut line = line.trim();
            while let Some(rest) = line
                .strip_prefix(MARKS)
                .filter(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
            {
                line = rest.trim_start();
            }
            line
        })
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// `text` as a web address: empty stays empty, `http` and `https` are the
/// only schemes, and one typed without a scheme gets `https://`. `None`
/// when it is not an address a browser would open: spaces, another
/// scheme, a user name, or no site with a dot in its name.
fn normalize_link(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        return Some(String::new());
    }
    if text.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    // A scheme is letters before the first `://`; one later in the text
    // (`example.com/r?to=https://…`) belongs to the path.
    let typed = text.split_once("://").filter(|(scheme, _)| {
        !scheme.is_empty()
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '+')
    });
    let (scheme, rest) = match typed {
        Some((scheme, rest)) if scheme.eq_ignore_ascii_case("https") => ("https", rest),
        Some((scheme, rest)) if scheme.eq_ignore_ascii_case("http") => ("http", rest),
        Some(_) => return None,
        None => ("https", text),
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    // A user name or a password has no place in a product's link.
    if authority.contains('@') {
        return None;
    }
    let host = match authority.rsplit_once(':') {
        Some((host, port)) => {
            if !port.chars().all(|c| c.is_ascii_digit()) || port.parse::<u16>().is_err() {
                return None;
            }
            host
        }
        None => authority,
    };
    let label = |label: &str| {
        !label.is_empty()
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label.chars().all(|c| c.is_alphanumeric() || c == '-')
    };
    let host = host.strip_suffix('.').unwrap_or(host);
    if !host.contains('.') || !host.split('.').all(label) {
        return None;
    }
    Some(format!("{scheme}://{rest}"))
}

/// A product in the user's library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Product {
    pub id: ProductId,
    pub owner: ProfileId,
    pub brief: ProductBrief,
    /// Hidden from the list and from new ads; its videos keep it.
    pub archived: bool,
}

/// Why a product cannot be deleted: video projects use it. Archiving
/// hides it instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{projects} video project(s) use the product")]
pub struct ProductInUse {
    pub projects: usize,
}

impl Product {
    pub fn new(owner: ProfileId, brief: ProductBrief) -> Self {
        Self {
            id: ProductId::new(),
            owner,
            brief,
            archived: false,
        }
    }

    /// Whether the product may be deleted, given how many video projects
    /// use it: a project keeps its product, so only an unused one goes.
    pub fn check_deletable(&self, projects: usize) -> Result<(), ProductInUse> {
        if projects == 0 {
            Ok(())
        } else {
            Err(ProductInUse { projects })
        }
    }
}

/// The shortest side of a product image, in pixels.
pub const PRODUCT_IMAGE_MIN_SIDE: u32 = 600;
/// The longest side of a product image or logo, in pixels.
pub const PRODUCT_IMAGE_MAX_SIDE: u32 = 6_000;
/// How many times one side of a product image may be the other: a picture
/// narrower or wider than this cannot be shown whole in a video frame at
/// a useful size.
pub const PRODUCT_IMAGE_MAX_RATIO: u32 = 3;
/// The largest product image or logo file.
pub const PRODUCT_IMAGE_MAX_BYTES: u64 = 10_000_000;
/// How many images a product holds, the logo aside.
pub const MAX_PRODUCT_IMAGES: usize = 3;
/// The shortest side of a logo, in pixels.
pub const PRODUCT_LOGO_MIN_SIDE: u32 = 64;
/// What a logo's longer side must reach, in pixels, to stay sharp on an
/// end card.
pub const PRODUCT_LOGO_MIN_LONG_SIDE: u32 = 200;
/// How many times one side of a logo may be the other: wordmarks are
/// wide.
pub const PRODUCT_LOGO_MAX_RATIO: u32 = 10;

/// Why a file cannot be a product image or logo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
pub enum ProductImageError {
    /// Not a PNG, JPEG or WebP file, or one whose size cannot be read.
    #[error("not a PNG, JPEG or WebP image")]
    NotAnImage,
    /// A logo that is not a PNG file.
    #[error("the logo is not a PNG image")]
    LogoNotPng,
    #[error("{width}×{height} px is too small")]
    TooSmall { width: u32, height: u32 },
    #[error("{width}×{height} px is too large")]
    TooLarge { width: u32, height: u32 },
    /// Too narrow or too wide.
    #[error("{width}×{height} px is too narrow or too wide")]
    Shape { width: u32, height: u32 },
    #[error("the file is {0} bytes")]
    FileTooLarge(u64),
}

/// A product's picture file, checked against the limits of what it is
/// for: the user's own file, byte for byte.
#[derive(Clone, PartialEq, Eq)]
pub struct ProductPicture {
    bytes: Vec<u8>,
    format: ImageFormat,
    width: u32,
    height: u32,
    transparent: bool,
}

impl std::fmt::Debug for ProductPicture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProductPicture")
            .field("bytes", &self.bytes.len())
            .field("format", &self.format)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("transparent", &self.transparent)
            .finish()
    }
}

impl ProductPicture {
    /// The picture in `bytes`, if it can be a product image: a PNG, JPEG
    /// or WebP of [`PRODUCT_IMAGE_MIN_SIDE`] to [`PRODUCT_IMAGE_MAX_SIDE`]
    /// pixels a side, neither side over [`PRODUCT_IMAGE_MAX_RATIO`] times
    /// the other, within [`PRODUCT_IMAGE_MAX_BYTES`].
    pub fn image(bytes: Vec<u8>) -> Result<Self, ProductImageError> {
        let picture = Self::read(bytes)?;
        picture.within(
            PRODUCT_IMAGE_MIN_SIDE,
            PRODUCT_IMAGE_MIN_SIDE,
            PRODUCT_IMAGE_MAX_RATIO,
        )?;
        Ok(picture)
    }

    /// The picture in `bytes`, if it can be a logo: a PNG of
    /// [`PRODUCT_LOGO_MIN_SIDE`] pixels a side or more, its longer side
    /// from [`PRODUCT_LOGO_MIN_LONG_SIDE`] to [`PRODUCT_IMAGE_MAX_SIDE`],
    /// neither side over [`PRODUCT_LOGO_MAX_RATIO`] times the other,
    /// within [`PRODUCT_IMAGE_MAX_BYTES`].
    pub fn logo(bytes: Vec<u8>) -> Result<Self, ProductImageError> {
        let picture = Self::read(bytes)?;
        if picture.format != ImageFormat::Png {
            return Err(ProductImageError::LogoNotPng);
        }
        picture.within(
            PRODUCT_LOGO_MIN_SIDE,
            PRODUCT_LOGO_MIN_LONG_SIDE,
            PRODUCT_LOGO_MAX_RATIO,
        )?;
        Ok(picture)
    }

    fn read(bytes: Vec<u8>) -> Result<Self, ProductImageError> {
        let format = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            ImageFormat::Png
        } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            ImageFormat::Jpeg
        } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
            ImageFormat::Webp
        } else {
            return Err(ProductImageError::NotAnImage);
        };
        let (width, height) = image_dimensions(&bytes).ok_or(ProductImageError::NotAnImage)?;
        if width == 0 || height == 0 {
            return Err(ProductImageError::NotAnImage);
        }
        let transparent = image_declares_transparency(&bytes);
        Ok(Self {
            bytes,
            format,
            width,
            height,
            transparent,
        })
    }

    fn within(
        &self,
        min_side: u32,
        min_long_side: u32,
        max_ratio: u32,
    ) -> Result<(), ProductImageError> {
        let (width, height) = (self.width, self.height);
        let (short, long) = (width.min(height), width.max(height));
        if short < min_side || long < min_long_side {
            return Err(ProductImageError::TooSmall { width, height });
        }
        if long > PRODUCT_IMAGE_MAX_SIDE {
            return Err(ProductImageError::TooLarge { width, height });
        }
        if u64::from(long) > u64::from(short) * u64::from(max_ratio) {
            return Err(ProductImageError::Shape { width, height });
        }
        let size = self.bytes.len() as u64;
        if size > PRODUCT_IMAGE_MAX_BYTES {
            return Err(ProductImageError::FileTooLarge(size));
        }
        Ok(())
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn format(&self) -> ImageFormat {
        self.format
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Whether the file says it carries transparency
    /// ([`image_declares_transparency`]).
    pub fn transparent(&self) -> bool {
        self.transparent
    }
}

/// What a product image shows, as the user marks it. Bardo does not
/// guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ProductImageKind {
    /// A photo: a physical product, its packaging, a place.
    #[default]
    Photo,
    /// A screenshot of an app or a site. It is only ever shown as it is:
    /// never drawn again, and never a reference for a drawing.
    Screen,
}

impl ProductImageKind {
    pub const ALL: [ProductImageKind; 2] = [ProductImageKind::Photo, ProductImageKind::Screen];

    /// Stable name stored in the database.
    pub fn code(self) -> &'static str {
        match self {
            ProductImageKind::Photo => "photo",
            ProductImageKind::Screen => "screen",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.code() == code)
    }
}

/// One picture of a product, imported by the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductImage {
    pub id: ProductImageId,
    pub product: ProductId,
    pub picture: ProductPicture,
    pub kind: ProductImageKind,
    /// When the user confirmed they may use the image and the brand it
    /// shows. No image is kept without it.
    pub confirmed_at: SystemTime,
}

impl ProductImage {
    pub fn new(
        product: ProductId,
        picture: ProductPicture,
        kind: ProductImageKind,
        confirmed_at: SystemTime,
    ) -> Self {
        Self {
            id: ProductImageId::new(),
            product,
            picture,
            kind,
            confirmed_at,
        }
    }

    /// Whether the picture is taken as already cut out of its background:
    /// a photo whose file carries transparency.
    pub fn cut_out(&self) -> bool {
        self.kind == ProductImageKind::Photo && self.picture.transparent()
    }

    /// The same image on another product (a duplicate), under an id of
    /// its own.
    pub fn for_product(&self, product: ProductId) -> Self {
        Self {
            id: ProductImageId::new(),
            product,
            ..self.clone()
        }
    }
}

/// A product's logo, imported by the user: a PNG.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductLogo {
    pub product: ProductId,
    pub picture: ProductPicture,
    /// When the user confirmed they may use the logo and the brand.
    pub confirmed_at: SystemTime,
}

impl ProductLogo {
    /// The same logo on another product (a duplicate).
    pub fn for_product(&self, product: ProductId) -> Self {
        Self {
            product,
            ..self.clone()
        }
    }
}

/// Persistence port for products, their images and logos.
pub trait ProductRepository: Send + Sync {
    /// The owner's products, archived ones included, ordered by name.
    fn list(&self, owner: ProfileId) -> Result<Vec<Product>, RepositoryError>;

    fn get(&self, id: ProductId) -> Result<Option<Product>, RepositoryError>;

    /// Inserts or updates the product.
    fn save(&self, product: &Product) -> Result<(), RepositoryError>;

    /// Deletes the product with its images and logo; one already gone is
    /// not an error.
    fn delete(&self, id: ProductId) -> Result<(), RepositoryError>;

    /// How many video projects use the product.
    fn projects_using(&self, id: ProductId) -> Result<usize, RepositoryError>;

    /// The product's images, in the order they were added.
    fn images(&self, product: ProductId) -> Result<Vec<ProductImage>, RepositoryError>;

    /// How many images the product has, without reading them.
    fn image_count(&self, product: ProductId) -> Result<usize, RepositoryError> {
        Ok(self.images(product)?.len())
    }

    /// Adds the image after the product's others. Returns `false`, and
    /// adds nothing, when the product already holds
    /// [`MAX_PRODUCT_IMAGES`].
    fn add_image(&self, image: &ProductImage) -> Result<bool, RepositoryError>;

    /// Marks an image as a photo or a screen. Returns whether the product
    /// has the image.
    fn set_image_kind(
        &self,
        product: ProductId,
        image: ProductImageId,
        kind: ProductImageKind,
    ) -> Result<bool, RepositoryError>;

    /// Removes an image. Returns whether the product had it.
    fn remove_image(
        &self,
        product: ProductId,
        image: ProductImageId,
    ) -> Result<bool, RepositoryError>;

    fn logo(&self, product: ProductId) -> Result<Option<ProductLogo>, RepositoryError>;

    /// Makes `logo` the product's logo, in place of any.
    fn save_logo(&self, logo: &ProductLogo) -> Result<(), RepositoryError>;

    /// Removes the product's logo. Returns whether it had one.
    fn remove_logo(&self, product: ProductId) -> Result<bool, RepositoryError>;

    /// The product's sheet, when it has one.
    fn sheet(&self, product: ProductId) -> Result<Option<ProductSheet>, RepositoryError>;

    /// Saves the sheet whole, in place of the product's earlier one: its
    /// text, its claims in their order and the pages it was read from.
    fn save_sheet(&self, sheet: &ProductSheet) -> Result<(), RepositoryError>;
}

impl<T: ProductRepository + ?Sized> ProductRepository for Arc<T> {
    fn list(&self, owner: ProfileId) -> Result<Vec<Product>, RepositoryError> {
        (**self).list(owner)
    }

    fn get(&self, id: ProductId) -> Result<Option<Product>, RepositoryError> {
        (**self).get(id)
    }

    fn save(&self, product: &Product) -> Result<(), RepositoryError> {
        (**self).save(product)
    }

    fn delete(&self, id: ProductId) -> Result<(), RepositoryError> {
        (**self).delete(id)
    }

    fn projects_using(&self, id: ProductId) -> Result<usize, RepositoryError> {
        (**self).projects_using(id)
    }

    fn images(&self, product: ProductId) -> Result<Vec<ProductImage>, RepositoryError> {
        (**self).images(product)
    }

    fn image_count(&self, product: ProductId) -> Result<usize, RepositoryError> {
        (**self).image_count(product)
    }

    fn add_image(&self, image: &ProductImage) -> Result<bool, RepositoryError> {
        (**self).add_image(image)
    }

    fn set_image_kind(
        &self,
        product: ProductId,
        image: ProductImageId,
        kind: ProductImageKind,
    ) -> Result<bool, RepositoryError> {
        (**self).set_image_kind(product, image, kind)
    }

    fn remove_image(
        &self,
        product: ProductId,
        image: ProductImageId,
    ) -> Result<bool, RepositoryError> {
        (**self).remove_image(product, image)
    }

    fn logo(&self, product: ProductId) -> Result<Option<ProductLogo>, RepositoryError> {
        (**self).logo(product)
    }

    fn save_logo(&self, logo: &ProductLogo) -> Result<(), RepositoryError> {
        (**self).save_logo(logo)
    }

    fn remove_logo(&self, product: ProductId) -> Result<bool, RepositoryError> {
        (**self).remove_logo(product)
    }

    fn sheet(&self, product: ProductId) -> Result<Option<ProductSheet>, RepositoryError> {
        (**self).sheet(product)
    }

    fn save_sheet(&self, sheet: &ProductSheet) -> Result<(), RepositoryError> {
        (**self).save_sheet(sheet)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn draft(name: &str) -> ProductDraft {
        ProductDraft {
            name: name.into(),
            description: "A notebook that never runs out of pages.".into(),
            ..ProductDraft::default()
        }
    }

    fn errors(draft: ProductDraft) -> Vec<ProductFieldError> {
        ProductBrief::validate(draft).unwrap_err()
    }

    #[test]
    fn a_name_and_a_description_are_enough() {
        let brief = ProductBrief::validate(draft("Caderno Infinito")).unwrap();
        assert_eq!(brief.name(), "Caderno Infinito");
        assert_eq!(brief.kind(), ProductKind::Digital);
        assert_eq!(brief.ownership(), ProductOwnership::Mine);
        assert!(brief.facts().is_empty());
        assert_eq!(brief.link(), "");
    }

    #[test]
    fn name_and_description_are_required() {
        assert_eq!(
            errors(ProductDraft {
                name: "  ".into(),
                description: "\n".into(),
                ..ProductDraft::default()
            }),
            [
                ProductFieldError::NameRequired,
                ProductFieldError::DescriptionRequired
            ]
        );
    }

    #[test]
    fn text_is_trimmed_and_limited_in_characters() {
        let brief = ProductBrief::validate(ProductDraft {
            audience: "  students \n".into(),
            offer: " 20% off until Friday ".into(),
            call_to_action: " Get yours ".into(),
            must_say: " Made in Brazil ".into(),
            never_say: " cheap ".into(),
            ..draft("  Notebook ")
        })
        .unwrap();
        assert_eq!(brief.name(), "Notebook");
        assert_eq!(brief.audience(), "students");
        assert_eq!(brief.offer(), "20% off until Friday");
        assert_eq!(brief.call_to_action(), "Get yours");
        assert_eq!(brief.must_say(), "Made in Brazil");
        assert_eq!(brief.never_say(), "cheap");

        let at_limit = "é".repeat(ProductBrief::MAX_NAME_CHARS);
        assert!(ProductBrief::validate(draft(&at_limit)).is_ok());
        let long = |n: usize| "x".repeat(n + 1);
        assert_eq!(
            errors(ProductDraft {
                name: long(ProductBrief::MAX_NAME_CHARS),
                description: long(ProductBrief::MAX_DESCRIPTION_CHARS),
                audience: long(ProductBrief::MAX_AUDIENCE_CHARS),
                offer: long(ProductBrief::MAX_OFFER_CHARS),
                call_to_action: long(ProductBrief::MAX_CALL_TO_ACTION_CHARS),
                must_say: long(ProductBrief::MAX_NOTE_CHARS),
                never_say: long(ProductBrief::MAX_NOTE_CHARS),
                ..ProductDraft::default()
            }),
            [
                ProductFieldError::NameTooLong,
                ProductFieldError::DescriptionTooLong,
                ProductFieldError::AudienceTooLong,
                ProductFieldError::OfferTooLong,
                ProductFieldError::CallToActionTooLong,
                ProductFieldError::MustSayTooLong,
                ProductFieldError::NeverSayTooLong,
            ]
        );
    }

    #[test]
    fn facts_are_one_per_line_trimmed_and_without_blank_lines() {
        let brief = ProductBrief::validate(ProductDraft {
            facts: "  Refillable pages \r\n\n- Ships in 2 days\n• 12-month warranty\n* A5 size\n-20% thinner than v1\n"
                .into(),
            ..draft("Notebook")
        })
        .unwrap();
        assert_eq!(
            brief.facts(),
            [
                "Refillable pages",
                "Ships in 2 days",
                "12-month warranty",
                "A5 size",
                // A dash that belongs to the statement stays.
                "-20% thinner than v1",
            ]
        );
    }

    #[test]
    fn every_leading_list_mark_goes_so_facts_read_back_the_same() {
        let facts = split_facts("- - Refillable\n•\tA5 size\n▪ · Ships fast\n- \n—");
        assert_eq!(facts, ["Refillable", "A5 size", "Ships fast"]);
        assert_eq!(split_facts(&facts.join("\n")), facts);
        // A statement that starts with a mark and no space keeps it.
        assert_eq!(
            split_facts("*new* formula\n–5 °C"),
            ["*new* formula", "–5 °C"]
        );
    }

    #[test]
    fn single_line_fields_read_line_breaks_as_spaces() {
        let brief = ProductBrief::validate(ProductDraft {
            offer: "20% off\nuntil Friday".into(),
            call_to_action: "Get\tyours\0".into(),
            ..draft("Refillable\r\nnotebook")
        })
        .unwrap();
        assert_eq!(brief.name(), "Refillable  notebook");
        assert_eq!(brief.offer(), "20% off until Friday");
        assert_eq!(brief.call_to_action(), "Get yours");
    }

    #[test]
    fn errors_come_in_the_order_the_form_shows_its_fields() {
        let long = |n: usize| "x".repeat(n + 1);
        assert_eq!(
            errors(ProductDraft {
                facts: long(ProductBrief::MAX_FACT_CHARS),
                link: "not a link".into(),
                call_to_action: long(ProductBrief::MAX_CALL_TO_ACTION_CHARS),
                must_say: long(ProductBrief::MAX_NOTE_CHARS),
                ..ProductDraft::default()
            }),
            [
                ProductFieldError::NameRequired,
                ProductFieldError::DescriptionRequired,
                ProductFieldError::LinkInvalid,
                ProductFieldError::CallToActionTooLong,
                ProductFieldError::FactTooLong,
                ProductFieldError::MustSayTooLong,
            ]
        );
    }

    #[test]
    fn a_fact_too_long_and_too_many_facts_are_one_error_each() {
        let long_fact = "x".repeat(ProductBrief::MAX_FACT_CHARS + 1);
        assert_eq!(
            errors(ProductDraft {
                facts: format!("{long_fact}\n{long_fact}"),
                ..draft("Notebook")
            }),
            [ProductFieldError::FactTooLong]
        );
        let many = vec!["a fact"; ProductBrief::MAX_FACTS + 1].join("\n");
        assert_eq!(
            errors(ProductDraft {
                facts: many,
                ..draft("Notebook")
            }),
            [ProductFieldError::TooManyFacts]
        );
        let at_limit = vec!["a fact"; ProductBrief::MAX_FACTS].join("\n");
        assert!(
            ProductBrief::validate(ProductDraft {
                facts: at_limit,
                ..draft("Notebook")
            })
            .is_ok()
        );
    }

    fn link(text: &str) -> Result<String, Vec<ProductFieldError>> {
        ProductBrief::validate(ProductDraft {
            link: text.into(),
            ..draft("Notebook")
        })
        .map(|brief| brief.link().to_owned())
    }

    #[test]
    fn a_link_is_a_web_address_and_gets_its_scheme() {
        assert_eq!(link("").unwrap(), "");
        assert_eq!(link("  example.com ").unwrap(), "https://example.com");
        assert_eq!(
            link("HTTPS://Loja.Example.com.br/caderno?cor=azul#fotos").unwrap(),
            "https://Loja.Example.com.br/caderno?cor=azul#fotos"
        );
        assert_eq!(
            link("http://example.com:8080/a").unwrap(),
            "http://example.com:8080/a"
        );
        assert_eq!(link("café.example").unwrap(), "https://café.example");
        // A scheme later in the text belongs to the path.
        assert_eq!(
            link("example.com/r?to=https://shop.example").unwrap(),
            "https://example.com/r?to=https://shop.example"
        );
        assert_eq!(
            link("example.com/@handle").unwrap(),
            "https://example.com/@handle"
        );
        assert_eq!(
            link("192.168.0.10:65535").unwrap(),
            "https://192.168.0.10:65535"
        );
    }

    #[test]
    fn a_link_that_is_not_a_web_address_is_refused() {
        for text in [
            "not a link",
            "ftp://example.com/file",
            "mailto:me@example.com",
            "https://user:secret@example.com",
            "https://localhost/page",
            "https://",
            "https://example..com",
            "https://-example.com",
            "https://example.com:port",
            "https://example.com:65536",
            "https://example.com:",
            "://example.com",
            "javascript:alert(1)",
            "example.com/a b",
        ] {
            assert_eq!(
                link(text).unwrap_err(),
                [ProductFieldError::LinkInvalid],
                "{text}"
            );
        }
        let long = format!(
            "https://example.com/{}",
            "x".repeat(ProductBrief::MAX_LINK_CHARS)
        );
        assert_eq!(link(&long).unwrap_err(), [ProductFieldError::LinkTooLong]);
    }

    #[test]
    fn a_valid_brief_round_trips_through_a_draft() {
        let brief = ProductBrief::validate(ProductDraft {
            name: "Notebook".into(),
            kind: ProductKind::Physical,
            description: "A notebook.".into(),
            audience: "Students".into(),
            facts: "Refillable\nA5 size".into(),
            offer: "20% off".into(),
            link: "example.com".into(),
            call_to_action: "Get yours".into(),
            ownership: ProductOwnership::Client,
            must_say: "Made in Brazil".into(),
            never_say: "cheap".into(),
        })
        .unwrap();
        assert_eq!(
            ProductBrief::validate(ProductDraft::from(&brief)),
            Ok(brief)
        );
    }

    #[test]
    fn copies_get_a_free_numbered_name() {
        let brief = ProductBrief::validate(draft("Notebook")).unwrap();
        let taken: HashSet<String> = ["notebook (copy)".to_owned()].into_iter().collect();
        assert_eq!(
            brief.copy_name("copy", |name| taken.contains(&name.to_lowercase())),
            "Notebook (copy 2)"
        );
        assert!(brief.same_name(" NOTEBOOK "));
    }

    #[test]
    fn codes_read_back() {
        for kind in ProductKind::ALL {
            assert_eq!(ProductKind::from_code(kind.code()), Some(kind));
        }
        for ownership in ProductOwnership::ALL {
            assert_eq!(
                ProductOwnership::from_code(ownership.code()),
                Some(ownership)
            );
        }
        for kind in ProductImageKind::ALL {
            assert_eq!(ProductImageKind::from_code(kind.code()), Some(kind));
        }
        assert_eq!(ProductKind::from_code("gadget"), None);
    }

    #[test]
    fn a_product_in_use_cannot_be_deleted() {
        let product = Product::new(
            ProfileId::new(),
            ProductBrief::validate(draft("Notebook")).unwrap(),
        );
        assert!(!product.archived);
        assert_eq!(product.check_deletable(0), Ok(()));
        assert_eq!(
            product.check_deletable(2),
            Err(ProductInUse { projects: 2 })
        );
    }

    /// A PNG header of this size: RGBA when `alpha`, RGB otherwise.
    fn png(width: u32, height: u32, alpha: bool) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        bytes.extend(width.to_be_bytes());
        bytes.extend(height.to_be_bytes());
        bytes.extend([8, if alpha { 6 } else { 2 }, 0, 0, 0]);
        bytes
    }

    fn jpeg(width: u16, height: u16) -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x11, 0x08];
        bytes.extend(height.to_be_bytes());
        bytes.extend(width.to_be_bytes());
        bytes
    }

    fn webp(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"RIFF\0\0\0\0WEBPVP8X\0\0\0\0".to_vec();
        bytes.extend([0x10, 0, 0, 0]);
        bytes.extend(&(width - 1).to_le_bytes()[..3]);
        bytes.extend(&(height - 1).to_le_bytes()[..3]);
        bytes
    }

    #[test]
    fn png_jpeg_and_webp_within_the_limits_are_product_images() {
        let picture = ProductPicture::image(png(1200, 1600, false)).unwrap();
        assert_eq!(picture.format(), ImageFormat::Png);
        assert_eq!((picture.width(), picture.height()), (1200, 1600));
        assert!(!picture.transparent());
        assert_eq!(
            ProductPicture::image(jpeg(600, 600)).unwrap().format(),
            ImageFormat::Jpeg
        );
        let webp = ProductPicture::image(webp(6000, 2000)).unwrap();
        assert_eq!(webp.format(), ImageFormat::Webp);
        assert!(webp.transparent());
    }

    #[test]
    fn the_file_is_kept_byte_for_byte() {
        let bytes = png(800, 800, true);
        let picture = ProductPicture::image(bytes.clone()).unwrap();
        assert_eq!(picture.bytes(), bytes);
    }

    #[test]
    fn an_image_out_of_the_limits_is_refused_with_the_reason() {
        let refused = |bytes: Vec<u8>| ProductPicture::image(bytes).unwrap_err();
        assert_eq!(refused(b"GIF89a".to_vec()), ProductImageError::NotAnImage);
        assert_eq!(
            refused(b"\x89PNG\r\n\x1a\n".to_vec()),
            ProductImageError::NotAnImage,
            "a header cut short"
        );
        assert_eq!(refused(png(0, 800, false)), ProductImageError::NotAnImage);
        assert_eq!(
            refused(png(599, 800, false)),
            ProductImageError::TooSmall {
                width: 599,
                height: 800
            }
        );
        assert_eq!(
            refused(png(6001, 4000, false)),
            ProductImageError::TooLarge {
                width: 6001,
                height: 4000
            }
        );
        assert_eq!(
            refused(png(600, 1801, false)),
            ProductImageError::Shape {
                width: 600,
                height: 1801
            }
        );
        assert!(ProductPicture::image(png(600, 1800, false)).is_ok());
        let mut heavy = png(800, 800, false);
        heavy.resize(PRODUCT_IMAGE_MAX_BYTES as usize + 1, 0);
        assert_eq!(
            refused(heavy),
            ProductImageError::FileTooLarge(PRODUCT_IMAGE_MAX_BYTES + 1)
        );
    }

    #[test]
    fn a_logo_is_a_png_and_may_be_small_and_wide() {
        assert!(ProductPicture::logo(png(800, 100, true)).is_ok());
        assert!(ProductPicture::logo(png(256, 256, true)).is_ok());
        let refused = |bytes: Vec<u8>| ProductPicture::logo(bytes).unwrap_err();
        assert_eq!(refused(jpeg(800, 800)), ProductImageError::LogoNotPng);
        assert_eq!(refused(webp(800, 800)), ProductImageError::LogoNotPng);
        assert_eq!(
            refused(png(640, 63, true)),
            ProductImageError::TooSmall {
                width: 640,
                height: 63
            }
        );
        assert_eq!(
            refused(png(199, 199, true)),
            ProductImageError::TooSmall {
                width: 199,
                height: 199
            }
        );
        assert_eq!(
            refused(png(1000, 99, true)),
            ProductImageError::Shape {
                width: 1000,
                height: 99
            }
        );
        assert_eq!(
            refused(png(6001, 1000, true)),
            ProductImageError::TooLarge {
                width: 6001,
                height: 1000
            }
        );
    }

    #[test]
    fn only_a_photo_with_transparency_is_taken_as_cut_out() {
        let product = ProductId::new();
        let at = SystemTime::UNIX_EPOCH;
        let image = |alpha: bool, kind: ProductImageKind| {
            ProductImage::new(
                product,
                ProductPicture::image(png(800, 800, alpha)).unwrap(),
                kind,
                at,
            )
        };
        assert!(image(true, ProductImageKind::Photo).cut_out());
        assert!(!image(false, ProductImageKind::Photo).cut_out());
        assert!(!image(true, ProductImageKind::Screen).cut_out());
    }

    #[test]
    fn a_copy_of_an_image_belongs_to_the_other_product_under_its_own_id() {
        let original = ProductImage::new(
            ProductId::new(),
            ProductPicture::image(png(800, 800, false)).unwrap(),
            ProductImageKind::Screen,
            SystemTime::UNIX_EPOCH,
        );
        let other = ProductId::new();
        let copy = original.for_product(other);
        assert_eq!(copy.product, other);
        assert_ne!(copy.id, original.id);
        assert_eq!(copy.picture, original.picture);
        assert_eq!(copy.kind, ProductImageKind::Screen);
        assert_eq!(copy.confirmed_at, original.confirmed_at);
    }
}
