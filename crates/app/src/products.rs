//! Product use cases (#273, docs/spec/marketing-videos.md): the profile's
//! library of things to advertise. A product is its brief, up to three
//! images and an optional logo, kept with it in the database. Products
//! are created, edited, duplicated, archived and deleted here; their
//! pictures are imported from files the user picks, after the user
//! confirms they may use them. Nothing here calls a model.

use std::io::Read as _;
use std::path::Path;
use std::time::SystemTime;

use bardo_domain::{
    MAX_PRODUCT_IMAGES, PRODUCT_IMAGE_MAX_BYTES, Product, ProductBrief, ProductDraft,
    ProductFieldError, ProductId, ProductImage, ProductImageError, ProductImageId,
    ProductImageKind, ProductLogo, ProductPicture, RepositoryError,
};

use crate::{AppError, Bardo, Text};

#[derive(Debug, thiserror::Error)]
pub enum ProductError {
    /// The draft breaks one or more field rules; the form shows each one.
    #[error("invalid product: {0:?}")]
    Invalid(Vec<ProductFieldError>),
    #[error("a product with this name already exists")]
    NameTaken,
    #[error("product not found")]
    NotFound,
    /// Video projects use the product: it can be archived, not deleted.
    #[error("{0} video project(s) use the product")]
    InUse(usize),
    /// A research or a writing of the product's sheet is under way: the
    /// product stays until it ends, so the run does not pay for a sheet
    /// nobody keeps.
    #[error("the product's sheet is being written")]
    SheetBusy,
    /// Ad angles are being proposed or ranked for the product: it stays
    /// until the job ends, so the job does not pay for angles nobody
    /// keeps.
    #[error("ad angles are being proposed for the product")]
    AnglesBusy,
    /// A job is drawing the product's concept image: it stays until the
    /// job ends, so the drawing is not paid for nothing.
    #[error("a concept image is being drawn for the product")]
    ConceptBusy,
    /// The user did not confirm they may use the image or logo and the
    /// brand it shows.
    #[error("the right to use the image was not confirmed")]
    NotConfirmed,
    /// The product already holds [`MAX_PRODUCT_IMAGES`].
    #[error("the product already has {MAX_PRODUCT_IMAGES} images")]
    TooManyImages,
    #[error(transparent)]
    Image(ProductImageError),
    /// The file could not be read.
    #[error("could not read the file: {0}")]
    Unreadable(String),
    #[error("image not found")]
    ImageNotFound,
    #[error("the product has no logo")]
    NoLogo,
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl ProductError {
    /// Message for the whole form. Field errors are shown next to their
    /// fields; a refused picture's reason goes under the message
    /// (`image_reason`).
    pub fn form_message(&self) -> Option<Text> {
        match self {
            ProductError::Invalid(_) => None,
            ProductError::NameTaken => Some(Text::ProductNameTaken),
            ProductError::NotFound => Some(Text::ProductNotFound),
            ProductError::InUse(_) => Some(Text::ProductInUse),
            ProductError::SheetBusy => Some(Text::SheetBusy),
            ProductError::AnglesBusy => Some(Text::AdAnglesBusy),
            ProductError::ConceptBusy => Some(Text::ConceptBusy),
            ProductError::NotConfirmed => Some(Text::ProductImageNotConfirmed),
            ProductError::TooManyImages => Some(Text::ProductTooManyImages),
            ProductError::Image(_) => Some(Text::ProductImageRefused),
            ProductError::Unreadable(_) => Some(Text::ProductImageUnreadable),
            ProductError::ImageNotFound => Some(Text::ProductImageNotFound),
            ProductError::NoLogo => Some(Text::ProductNoLogo),
            ProductError::Repository(_) => Some(Text::ProductNotSaved),
        }
    }

    pub fn field_errors(&self) -> &[ProductFieldError] {
        match self {
            ProductError::Invalid(errors) => errors,
            _ => &[],
        }
    }

    /// Why a picture was refused, when it was.
    pub fn image_reason(&self) -> Option<ProductImageError> {
        match self {
            ProductError::Image(reason) => Some(*reason),
            _ => None,
        }
    }
}

/// Reads a picture file the user picked, no more than one byte past the
/// largest file a product picture may be.
pub(crate) fn read_picture(path: &Path) -> Result<Vec<u8>, ProductError> {
    let unreadable = |error: std::io::Error| {
        tracing::warn!(%error, "could not read a product picture");
        ProductError::Unreadable(error.to_string())
    };
    let file = std::fs::File::open(path).map_err(unreadable)?;
    let size = file.metadata().map_err(unreadable)?.len();
    if size > PRODUCT_IMAGE_MAX_BYTES {
        return Err(ProductError::Image(ProductImageError::FileTooLarge(size)));
    }
    let mut bytes = Vec::new();
    file
        // One byte past the limit tells "too large" from "at the limit",
        // should the file grow meanwhile.
        .take(PRODUCT_IMAGE_MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(unreadable)?;
    Ok(bytes)
}

impl Bardo {
    /// The profile's products, archived ones included, ordered by name.
    pub fn products(&self) -> Result<Vec<Product>, AppError> {
        Ok(self.products.list(self.profile.id)?)
    }

    fn own_product(&self, id: ProductId) -> Result<Product, ProductError> {
        self.products
            .get(id)?
            .filter(|product| product.owner == self.profile.id)
            .ok_or(ProductError::NotFound)
    }

    fn ensure_product_name_free(
        &self,
        name: &str,
        editing: Option<ProductId>,
    ) -> Result<(), ProductError> {
        let taken = self
            .products
            .list(self.profile.id)?
            .iter()
            .any(|other| Some(other.id) != editing && other.brief.same_name(name));
        if taken {
            Err(ProductError::NameTaken)
        } else {
            Ok(())
        }
    }

    pub fn create_product(&self, draft: ProductDraft) -> Result<Product, ProductError> {
        let brief = ProductBrief::validate(draft).map_err(ProductError::Invalid)?;
        self.ensure_product_name_free(brief.name(), None)?;
        let product = Product::new(self.profile.id, brief);
        self.products.save(&product)?;
        tracing::info!("created a product");
        Ok(product)
    }

    /// Saves changes to a product's brief. Its images and logo stay.
    pub fn update_product(
        &self,
        id: ProductId,
        draft: ProductDraft,
    ) -> Result<Product, ProductError> {
        let mut product = self.own_product(id)?;
        let brief = ProductBrief::validate(draft).map_err(ProductError::Invalid)?;
        if brief == product.brief {
            return Ok(product);
        }
        self.ensure_product_name_free(brief.name(), Some(id))?;
        product.brief = brief;
        self.products.save(&product)?;
        // A fact typed since goes on the sheet, when there is one.
        self.take_brief_facts(&product);
        Ok(product)
    }

    /// A new product with the same brief, images, logo and sheet, named
    /// as a copy, to vary without touching the original. The copy is not
    /// archived.
    pub fn duplicate_product(&self, id: ProductId) -> Result<Product, ProductError> {
        let original = self.own_product(id)?;
        let taken = self.products.list(self.profile.id)?;
        let name = original
            .brief
            .copy_name(&self.text(Text::ProductCopySuffix), |name| {
                taken.iter().any(|other| other.brief.same_name(name))
            });
        let brief = ProductBrief::validate(ProductDraft {
            name,
            ..ProductDraft::from(&original.brief)
        })
        .map_err(ProductError::Invalid)?;
        let copy = Product::new(self.profile.id, brief);
        self.products.save(&copy)?;
        // The pictures come along, each with the user's confirmation. The
        // copy is made either way: copying again would make a second one.
        let copied = self.products.images(original.id).and_then(|images| {
            for image in images {
                self.products.add_image(&image.for_product(copy.id))?;
            }
            if let Some(logo) = self.products.logo(original.id)? {
                self.products.save_logo(&logo.for_product(copy.id))?;
            }
            // Its kept concept image too (#280): it was paid for once.
            if let Some(concept) = self.products.concept(original.id)? {
                self.products.save_concept(&concept.for_product(copy.id))?;
            }
            // The sheet too, with every claim as the user left it: the
            // research was paid for once.
            match self.products.sheet(original.id)? {
                Some(sheet) => self.products.save_sheet(&sheet.for_product(
                    copy.id,
                    crate::publications::whole_millis(SystemTime::now()),
                )),
                None => Ok(()),
            }
        });
        if let Err(error) = copied {
            tracing::warn!(%error, "could not copy a product's pictures and sheet");
        }
        Ok(copy)
    }

    /// Archives the product, hiding it from the list and from new ads, or
    /// brings it back. Its videos keep it either way.
    pub fn set_product_archived(
        &self,
        id: ProductId,
        archived: bool,
    ) -> Result<Product, ProductError> {
        let mut product = self.own_product(id)?;
        if product.archived != archived {
            product.archived = archived;
            self.products.save(&product)?;
        }
        Ok(product)
    }

    /// Deletes the product with its images, logo and sheet. Refused while
    /// a video project uses it (archiving hides it instead) and while a
    /// job fills its sheet, proposes or ranks its ad angles, or draws its
    /// concept image.
    pub fn delete_product(&self, id: ProductId) -> Result<(), ProductError> {
        let product = self.own_product(id)?;
        if self.sheet_is_busy(id) {
            return Err(ProductError::SheetBusy);
        }
        if self.ad_angles_busy(id) {
            return Err(ProductError::AnglesBusy);
        }
        if self.work_busy(&crate::jobs::Work::Concept(id.to_string())) {
            return Err(ProductError::ConceptBusy);
        }
        let projects = self.products.projects_using(id)?;
        product
            .check_deletable(projects)
            .map_err(|used| ProductError::InUse(used.projects))?;
        self.products.delete(id)?;
        tracing::info!("deleted a product");
        Ok(())
    }

    /// The product's images, in order.
    pub fn product_images(&self, id: ProductId) -> Result<Vec<ProductImage>, ProductError> {
        self.own_product(id)?;
        Ok(self.products.images(id)?)
    }

    /// The product's logo, if it has one.
    pub fn product_logo(&self, id: ProductId) -> Result<Option<ProductLogo>, ProductError> {
        self.own_product(id)?;
        Ok(self.products.logo(id)?)
    }

    /// Adds the picture at `path` to the product's images, marked as a
    /// photo, once the user confirmed (`confirmed`) they may use it and
    /// the brand it shows. The user's file is kept as it is.
    pub fn import_product_image(
        &self,
        id: ProductId,
        path: &Path,
        confirmed: bool,
    ) -> Result<ProductImage, ProductError> {
        self.own_product(id)?;
        if !confirmed {
            return Err(ProductError::NotConfirmed);
        }
        let picture = ProductPicture::image(read_picture(path)?).map_err(ProductError::Image)?;
        let image = ProductImage::new(
            id,
            picture,
            ProductImageKind::Photo,
            crate::publications::whole_millis(SystemTime::now()),
        );
        if !self.products.add_image(&image)? {
            return Err(ProductError::TooManyImages);
        }
        tracing::info!("imported a product image");
        Ok(image)
    }

    /// Marks one of the product's images as a photo or a screen.
    pub fn set_product_image_kind(
        &self,
        id: ProductId,
        image: ProductImageId,
        kind: ProductImageKind,
    ) -> Result<(), ProductError> {
        self.own_product(id)?;
        // Only a drawing Bardo made is a concept image: no imported
        // picture is found to mark as one.
        if !ProductImageKind::ALL.contains(&kind) {
            return Err(ProductError::ImageNotFound);
        }
        if self.products.set_image_kind(id, image, kind)? {
            Ok(())
        } else {
            Err(ProductError::ImageNotFound)
        }
    }

    /// Removes one of the product's images; the others move up.
    pub fn remove_product_image(
        &self,
        id: ProductId,
        image: ProductImageId,
    ) -> Result<(), ProductError> {
        self.own_product(id)?;
        if self.products.remove_image(id, image)? {
            Ok(())
        } else {
            Err(ProductError::ImageNotFound)
        }
    }

    /// Makes the PNG at `path` the product's logo, in place of any, once
    /// the user confirmed (`confirmed`) they may use it.
    pub fn import_product_logo(
        &self,
        id: ProductId,
        path: &Path,
        confirmed: bool,
    ) -> Result<ProductLogo, ProductError> {
        self.own_product(id)?;
        if !confirmed {
            return Err(ProductError::NotConfirmed);
        }
        let picture = ProductPicture::logo(read_picture(path)?).map_err(ProductError::Image)?;
        let logo = ProductLogo {
            product: id,
            picture,
            confirmed_at: crate::publications::whole_millis(SystemTime::now()),
        };
        self.products.save_logo(&logo)?;
        tracing::info!("imported a product logo");
        // The ads' end cards show it (#281).
        self.refresh_end_cards_of(id);
        Ok(logo)
    }

    /// When a picture's use was confirmed, as the user reads dates.
    pub fn product_time_text(&self, at: SystemTime) -> String {
        self.catalog
            .date_time_here(&bardo_domain::local_time(at, &self.zone))
    }

    pub fn remove_product_logo(&self, id: ProductId) -> Result<(), ProductError> {
        self.own_product(id)?;
        if self.products.remove_logo(id)? {
            self.refresh_end_cards_of(id);
            Ok(())
        } else {
            Err(ProductError::NoLogo)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use bardo_domain::{
        ImageFormat, PRODUCT_IMAGE_MAX_BYTES, ProductKind, ProductOwnership, ProductRepository,
        ProfileId,
    };
    use bardo_storage::{Database, MemorySecretStore};

    use super::*;
    use crate::{Repositories, testing};

    struct Harness {
        db: Arc<Database>,
        dir: tempfile::TempDir,
    }

    impl Harness {
        fn new() -> Self {
            Self {
                db: Arc::new(Database::open_in_memory().unwrap()),
                dir: tempfile::tempdir().unwrap(),
            }
        }

        fn start(&self) -> Bardo {
            self.start_with(Arc::clone(&self.db) as _)
        }

        fn start_with(&self, products: Arc<dyn ProductRepository>) -> Bardo {
            let repositories = Repositories {
                products,
                ..Repositories::shared(Arc::clone(&self.db), Arc::new(MemorySecretStore::default()))
            };
            let mut app = Bardo::start(repositories, testing::providers(), Some("en-US")).unwrap();
            app.accept_privacy_policy().unwrap();
            app
        }

        /// Writes `bytes` to a file of the harness's folder.
        fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
            let path = self.dir.path().join(name);
            std::fs::write(&path, bytes).unwrap();
            path
        }
    }

    /// A PNG header of this size, RGBA when `alpha`, padded with `mark` so
    /// two files differ.
    fn png(width: u32, height: u32, alpha: bool, mark: u8) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        bytes.extend(width.to_be_bytes());
        bytes.extend(height.to_be_bytes());
        bytes.extend([8, if alpha { 6 } else { 2 }, 0, 0, 0, mark]);
        bytes
    }

    fn jpeg(width: u16, height: u16) -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x11, 0x08];
        bytes.extend(height.to_be_bytes());
        bytes.extend(width.to_be_bytes());
        bytes
    }

    fn draft(name: &str) -> ProductDraft {
        ProductDraft {
            name: name.into(),
            kind: ProductKind::Physical,
            description: "A notebook whose pages are refilled.".into(),
            audience: "Students".into(),
            facts: "Refillable pages\n\nA5 size\n".into(),
            offer: "20% off until Friday".into(),
            link: "example.com/notebook".into(),
            call_to_action: "Get yours".into(),
            ownership: ProductOwnership::Client,
            must_say: "Made in Brazil".into(),
            never_say: "cheap".into(),
        }
    }

    fn names(app: &Bardo) -> Vec<String> {
        app.products()
            .unwrap()
            .iter()
            .map(|product| product.brief.name().to_owned())
            .collect()
    }

    #[test]
    fn a_new_profile_has_no_products() {
        let h = Harness::new();
        assert!(h.start().products().unwrap().is_empty());
    }

    #[test]
    fn a_product_is_created_from_a_valid_brief_and_kept() {
        let h = Harness::new();
        let app = h.start();
        let product = app.create_product(draft("  Notebook ")).unwrap();
        assert_eq!(product.brief.name(), "Notebook");
        assert_eq!(product.brief.facts(), ["Refillable pages", "A5 size"]);
        assert_eq!(product.brief.link(), "https://example.com/notebook");
        assert!(!product.archived);
        // Kept for the next start.
        assert_eq!(h.start().products().unwrap(), [product]);
    }

    #[test]
    fn an_invalid_brief_names_every_problem_and_saves_nothing() {
        let h = Harness::new();
        let app = h.start();
        let error = app
            .create_product(ProductDraft {
                name: String::new(),
                description: String::new(),
                link: "not a link".into(),
                ..ProductDraft::default()
            })
            .unwrap_err();
        assert_eq!(
            error.field_errors(),
            [
                ProductFieldError::NameRequired,
                ProductFieldError::DescriptionRequired,
                ProductFieldError::LinkInvalid,
            ]
        );
        assert_eq!(error.form_message(), None);
        assert!(app.products().unwrap().is_empty());
    }

    #[test]
    fn names_are_unique_whatever_their_case() {
        let h = Harness::new();
        let app = h.start();
        let first = app.create_product(draft("Notebook")).unwrap();
        let other = app.create_product(draft("Pen")).unwrap();
        assert!(matches!(
            app.create_product(draft("NOTEBOOK")),
            Err(ProductError::NameTaken)
        ));
        assert!(matches!(
            app.update_product(other.id, draft("notebook")),
            Err(ProductError::NameTaken)
        ));
        // A product keeps its own name, in another case too.
        let renamed = app.update_product(first.id, draft("NoteBook")).unwrap();
        assert_eq!(renamed.brief.name(), "NoteBook");
    }

    #[test]
    fn editing_changes_the_brief_and_keeps_the_pictures() {
        let h = Harness::new();
        let app = h.start();
        let product = app.create_product(draft("Notebook")).unwrap();
        let photo = h.file("photo.png", &png(900, 1200, false, 1));
        app.import_product_image(product.id, &photo, true).unwrap();
        let edited = app
            .update_product(
                product.id,
                ProductDraft {
                    kind: ProductKind::Idea,
                    ownership: ProductOwnership::Mine,
                    ..draft("Notebook")
                },
            )
            .unwrap();
        assert_eq!(edited.brief.kind(), ProductKind::Idea);
        assert_eq!(edited.brief.ownership(), ProductOwnership::Mine);
        assert_eq!(app.product_images(product.id).unwrap().len(), 1);
        assert!(matches!(
            app.update_product(ProductId::new(), draft("Other")),
            Err(ProductError::NotFound)
        ));
    }

    #[test]
    fn an_image_is_kept_byte_for_byte_as_a_photo_with_its_confirmation() {
        let h = Harness::new();
        let app = h.start();
        let product = app.create_product(draft("Notebook")).unwrap();
        let bytes = png(900, 1200, true, 1);
        let path = h.file("cutout.png", &bytes);
        let before = SystemTime::now() - std::time::Duration::from_secs(1);
        let image = app.import_product_image(product.id, &path, true).unwrap();
        assert_eq!(image.picture.bytes(), bytes);
        assert_eq!(image.picture.format(), ImageFormat::Png);
        assert_eq!(image.kind, ProductImageKind::Photo);
        assert!(image.cut_out(), "a photo with transparency is cut out");
        assert!(image.confirmed_at >= before);
        assert_eq!(
            app.product_images(product.id).unwrap(),
            std::slice::from_ref(&image)
        );
        // The user's file is untouched.
        assert_eq!(std::fs::read(&path).unwrap(), bytes);

        // The user marks it a screen, and back.
        app.set_product_image_kind(product.id, image.id, ProductImageKind::Screen)
            .unwrap();
        let marked = &app.product_images(product.id).unwrap()[0];
        assert_eq!(marked.kind, ProductImageKind::Screen);
        assert!(!marked.cut_out());
        assert!(matches!(
            app.set_product_image_kind(product.id, ProductImageId::new(), ProductImageKind::Photo),
            Err(ProductError::ImageNotFound)
        ));
    }

    #[test]
    fn no_image_is_saved_without_the_confirmation() {
        let h = Harness::new();
        let app = h.start();
        let product = app.create_product(draft("Notebook")).unwrap();
        let path = h.file("photo.png", &png(900, 1200, false, 1));
        let error = app
            .import_product_image(product.id, &path, false)
            .unwrap_err();
        assert!(matches!(error, ProductError::NotConfirmed));
        assert_eq!(error.form_message(), Some(Text::ProductImageNotConfirmed));
        let logo = h.file("logo.png", &png(800, 200, true, 2));
        assert!(matches!(
            app.import_product_logo(product.id, &logo, false),
            Err(ProductError::NotConfirmed)
        ));
        assert!(app.product_images(product.id).unwrap().is_empty());
        assert_eq!(app.product_logo(product.id).unwrap(), None);
    }

    #[test]
    fn an_image_out_of_the_limits_or_unreadable_is_refused_with_the_reason() {
        let h = Harness::new();
        let app = h.start();
        let product = app.create_product(draft("Notebook")).unwrap();
        let refused = |name: &str, bytes: &[u8]| {
            app.import_product_image(product.id, &h.file(name, bytes), true)
                .unwrap_err()
        };
        let small = refused("small.png", &png(599, 900, false, 1));
        assert_eq!(
            small.image_reason(),
            Some(ProductImageError::TooSmall {
                width: 599,
                height: 900
            })
        );
        assert_eq!(small.form_message(), Some(Text::ProductImageRefused));
        assert_eq!(
            refused("wide.jpg", &jpeg(3000, 900)).image_reason(),
            Some(ProductImageError::Shape {
                width: 3000,
                height: 900
            })
        );
        assert_eq!(
            refused("anim.gif", b"GIF89a....").image_reason(),
            Some(ProductImageError::NotAnImage)
        );
        let mut heavy = png(900, 900, false, 1);
        heavy.resize(PRODUCT_IMAGE_MAX_BYTES as usize + 1, 0);
        assert_eq!(
            refused("heavy.png", &heavy).image_reason(),
            Some(ProductImageError::FileTooLarge(PRODUCT_IMAGE_MAX_BYTES + 1))
        );
        let missing = app
            .import_product_image(product.id, &h.dir.path().join("gone.png"), true)
            .unwrap_err();
        assert!(matches!(missing, ProductError::Unreadable(_)));
        assert!(app.product_images(product.id).unwrap().is_empty());
    }

    #[test]
    fn a_fourth_image_is_refused() {
        let h = Harness::new();
        let app = h.start();
        let product = app.create_product(draft("Notebook")).unwrap();
        let images: Vec<_> = (1..=3)
            .map(|mark| {
                let path = h.file(&format!("{mark}.png"), &png(900, 900, false, mark));
                app.import_product_image(product.id, &path, true).unwrap()
            })
            .collect();
        let fourth = h.file("4.png", &png(900, 900, false, 4));
        let error = app
            .import_product_image(product.id, &fourth, true)
            .unwrap_err();
        assert!(matches!(error, ProductError::TooManyImages));
        assert_eq!(error.form_message(), Some(Text::ProductTooManyImages));

        // Removing one makes room again, at the end.
        app.remove_product_image(product.id, images[0].id).unwrap();
        assert!(matches!(
            app.remove_product_image(product.id, images[0].id),
            Err(ProductError::ImageNotFound)
        ));
        let added = app.import_product_image(product.id, &fourth, true).unwrap();
        let ids: Vec<_> = app
            .product_images(product.id)
            .unwrap()
            .into_iter()
            .map(|image| image.id)
            .collect();
        assert_eq!(ids, [images[1].id, images[2].id, added.id]);
    }

    #[test]
    fn a_logo_is_a_png_replaced_in_place_and_removed() {
        let h = Harness::new();
        let app = h.start();
        let product = app.create_product(draft("Notebook")).unwrap();
        assert!(matches!(
            app.remove_product_logo(product.id),
            Err(ProductError::NoLogo)
        ));
        let jpeg_logo = h.file("logo.jpg", &jpeg(800, 800));
        assert_eq!(
            app.import_product_logo(product.id, &jpeg_logo, true)
                .unwrap_err()
                .image_reason(),
            Some(ProductImageError::LogoNotPng)
        );
        let first = app
            .import_product_logo(product.id, &h.file("a.png", &png(800, 200, true, 1)), true)
            .unwrap();
        let second = app
            .import_product_logo(product.id, &h.file("b.png", &png(300, 300, true, 2)), true)
            .unwrap();
        assert_ne!(first.picture, second.picture);
        assert_eq!(app.product_logo(product.id).unwrap(), Some(second));
        app.remove_product_logo(product.id).unwrap();
        assert_eq!(app.product_logo(product.id).unwrap(), None);
    }

    #[test]
    fn a_product_without_images_is_allowed_for_every_kind() {
        let h = Harness::new();
        let app = h.start();
        for (index, kind) in ProductKind::ALL.into_iter().enumerate() {
            let product = app
                .create_product(ProductDraft {
                    kind,
                    ..draft(&format!("Product {index}"))
                })
                .unwrap();
            assert!(app.product_images(product.id).unwrap().is_empty());
        }
    }

    #[test]
    fn a_duplicate_copies_the_brief_images_and_logo_under_a_free_name() {
        let h = Harness::new();
        let app = h.start();
        let product = app.create_product(draft("Notebook")).unwrap();
        let photo = app
            .import_product_image(
                product.id,
                &h.file("photo.png", &png(900, 1200, false, 1)),
                true,
            )
            .unwrap();
        app.set_product_image_kind(product.id, photo.id, ProductImageKind::Screen)
            .unwrap();
        let logo = app
            .import_product_logo(
                product.id,
                &h.file("logo.png", &png(800, 200, true, 2)),
                true,
            )
            .unwrap();
        app.set_product_archived(product.id, true).unwrap();

        let copy = app.duplicate_product(product.id).unwrap();
        assert_eq!(copy.brief.name(), "Notebook (copy)");
        assert_eq!(copy.brief.facts(), product.brief.facts());
        assert_eq!(copy.brief.ownership(), ProductOwnership::Client);
        assert!(!copy.archived, "a copy is made to be used");
        let images = app.product_images(copy.id).unwrap();
        assert_eq!(images.len(), 1);
        assert_ne!(images[0].id, photo.id);
        assert_eq!(images[0].picture, photo.picture);
        assert_eq!(images[0].kind, ProductImageKind::Screen);
        assert_eq!(images[0].confirmed_at, photo.confirmed_at);
        assert_eq!(
            app.product_logo(copy.id).unwrap().map(|l| l.picture),
            Some(logo.picture)
        );
        let again = app.duplicate_product(product.id).unwrap();
        assert_eq!(again.brief.name(), "Notebook (copy 2)");
        // The original keeps its own.
        assert_eq!(app.product_images(product.id).unwrap().len(), 1);
    }

    #[test]
    fn archiving_hides_and_restoring_brings_back() {
        let h = Harness::new();
        let app = h.start();
        let product = app.create_product(draft("Notebook")).unwrap();
        assert!(app.set_product_archived(product.id, true).unwrap().archived);
        assert!(app.products().unwrap()[0].archived);
        assert!(
            !app.set_product_archived(product.id, false)
                .unwrap()
                .archived
        );
        assert!(!app.products().unwrap()[0].archived);
        assert_eq!(names(&app), ["Notebook"]);
    }

    #[test]
    fn deleting_an_unused_product_takes_its_pictures() {
        let h = Harness::new();
        let app = h.start();
        let product = app.create_product(draft("Notebook")).unwrap();
        app.import_product_image(
            product.id,
            &h.file("photo.png", &png(900, 1200, false, 1)),
            true,
        )
        .unwrap();
        app.delete_product(product.id).unwrap();
        assert!(app.products().unwrap().is_empty());
        assert!(h.db.images(product.id).unwrap().is_empty());
        assert!(matches!(
            app.delete_product(product.id),
            Err(ProductError::NotFound)
        ));
    }

    /// The database, saying that video projects use every product.
    struct UsedProducts(Arc<Database>);

    impl ProductRepository for UsedProducts {
        fn list(&self, owner: ProfileId) -> Result<Vec<Product>, RepositoryError> {
            ProductRepository::list(&*self.0, owner)
        }
        fn get(&self, id: ProductId) -> Result<Option<Product>, RepositoryError> {
            ProductRepository::get(&*self.0, id)
        }
        fn save(&self, product: &Product) -> Result<(), RepositoryError> {
            ProductRepository::save(&*self.0, product)
        }
        fn delete(&self, id: ProductId) -> Result<(), RepositoryError> {
            ProductRepository::delete(&*self.0, id)
        }
        fn projects_using(&self, _id: ProductId) -> Result<usize, RepositoryError> {
            Ok(2)
        }
        fn sheet(
            &self,
            product: ProductId,
        ) -> Result<Option<bardo_domain::ProductSheet>, RepositoryError> {
            self.0.sheet(product)
        }
        fn save_sheet(&self, sheet: &bardo_domain::ProductSheet) -> Result<(), RepositoryError> {
            self.0.save_sheet(sheet)
        }
        fn images(&self, product: ProductId) -> Result<Vec<ProductImage>, RepositoryError> {
            self.0.images(product)
        }
        fn add_image(&self, image: &ProductImage) -> Result<bool, RepositoryError> {
            self.0.add_image(image)
        }
        fn set_image_kind(
            &self,
            product: ProductId,
            image: ProductImageId,
            kind: ProductImageKind,
        ) -> Result<bool, RepositoryError> {
            self.0.set_image_kind(product, image, kind)
        }
        fn remove_image(
            &self,
            product: ProductId,
            image: ProductImageId,
        ) -> Result<bool, RepositoryError> {
            self.0.remove_image(product, image)
        }
        fn logo(&self, product: ProductId) -> Result<Option<ProductLogo>, RepositoryError> {
            self.0.logo(product)
        }
        fn save_logo(&self, logo: &ProductLogo) -> Result<(), RepositoryError> {
            self.0.save_logo(logo)
        }
        fn remove_logo(&self, product: ProductId) -> Result<bool, RepositoryError> {
            self.0.remove_logo(product)
        }
        fn concept(
            &self,
            product: ProductId,
        ) -> Result<Option<bardo_domain::ProductConcept>, RepositoryError> {
            self.0.concept(product)
        }
        fn pending_concept(
            &self,
            product: ProductId,
        ) -> Result<Option<bardo_domain::ProductConcept>, RepositoryError> {
            self.0.pending_concept(product)
        }
        fn save_pending_concept(
            &self,
            concept: &bardo_domain::ProductConcept,
        ) -> Result<(), RepositoryError> {
            self.0.save_pending_concept(concept)
        }
        fn keep_pending_concept(&self, product: ProductId) -> Result<bool, RepositoryError> {
            self.0.keep_pending_concept(product)
        }
        fn remove_pending_concept(&self, product: ProductId) -> Result<bool, RepositoryError> {
            self.0.remove_pending_concept(product)
        }
        fn save_concept(
            &self,
            concept: &bardo_domain::ProductConcept,
        ) -> Result<(), RepositoryError> {
            self.0.save_concept(concept)
        }
    }

    #[test]
    fn a_product_a_project_uses_is_not_deleted_and_can_be_archived() {
        let h = Harness::new();
        let app = h.start_with(Arc::new(UsedProducts(Arc::clone(&h.db))));
        let product = app.create_product(draft("Notebook")).unwrap();
        let error = app.delete_product(product.id).unwrap_err();
        assert!(matches!(error, ProductError::InUse(2)));
        assert_eq!(error.form_message(), Some(Text::ProductInUse));
        assert_eq!(names(&app), ["Notebook"]);
        assert!(app.set_product_archived(product.id, true).unwrap().archived);
    }
}
