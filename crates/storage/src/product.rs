use bardo_domain::{
    MAX_PRODUCT_IMAGES, Product, ProductBrief, ProductDraft, ProductId, ProductImage,
    ProductImageId, ProductImageKind, ProductKind, ProductLogo, ProductOwnership, ProductPicture,
    ProductRepository, ProfileId, RepositoryError,
};
use rusqlite::{OptionalExtension, Row, params};
use uuid::Uuid;

use crate::{Database, boxed, from_unix_millis, to_unix_millis};

const SELECT_PRODUCT: &str = "SELECT id, profile_id, name, kind, description, audience, facts, \
     offer, link, call_to_action, ownership, must_say, never_say, archived FROM product";

/// A product row as stored, before domain validation.
struct ProductRow {
    id: String,
    profile_id: String,
    kind: String,
    ownership: String,
    /// Name, description, audience, facts, offer, link, call to action,
    /// must say, never say.
    text: [String; 9],
    archived: bool,
}

impl ProductRow {
    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            profile_id: row.get(1)?,
            kind: row.get(3)?,
            ownership: row.get(10)?,
            text: [
                row.get(2)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
                row.get(9)?,
                row.get(11)?,
                row.get(12)?,
            ],
            archived: row.get(13)?,
        })
    }

    /// Rebuilds the product through domain validation, so a row edited
    /// outside the app cannot smuggle in an invalid brief.
    fn into_product(self) -> Result<Product, RepositoryError> {
        let invalid = |what: String| boxed(InvalidRow(what));
        let kind = ProductKind::from_code(&self.kind)
            .ok_or_else(|| invalid(format!("kind {:?}", self.kind)))?;
        let ownership = ProductOwnership::from_code(&self.ownership)
            .ok_or_else(|| invalid(format!("ownership {:?}", self.ownership)))?;
        let [
            name,
            description,
            audience,
            facts,
            offer,
            link,
            call_to_action,
            must_say,
            never_say,
        ] = self.text;
        let brief = ProductBrief::validate(ProductDraft {
            name,
            kind,
            description,
            audience,
            facts,
            offer,
            link,
            call_to_action,
            ownership,
            must_say,
            never_say,
        })
        .map_err(|errors| invalid(format!("{errors:?}")))?;
        Ok(Product {
            id: ProductId::from(Uuid::parse_str(&self.id).map_err(boxed)?),
            owner: ProfileId::from(Uuid::parse_str(&self.profile_id).map_err(boxed)?),
            brief,
            archived: self.archived,
        })
    }
}

#[derive(Debug, thiserror::Error)]
#[error("stored product is invalid: {0}")]
struct InvalidRow(String);

/// An image row as stored, before domain checks.
struct ImageRow {
    id: String,
    product_id: String,
    kind: String,
    image: Vec<u8>,
    confirmed_at: i64,
}

impl ImageRow {
    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            product_id: row.get(1)?,
            kind: row.get(2)?,
            image: row.get(3)?,
            confirmed_at: row.get(4)?,
        })
    }

    /// Rebuilds the image through the domain's checks, so a file changed
    /// outside the app cannot pass for one within the limits.
    fn into_image(self) -> Result<ProductImage, RepositoryError> {
        Ok(ProductImage {
            id: ProductImageId::from(Uuid::parse_str(&self.id).map_err(boxed)?),
            product: ProductId::from(Uuid::parse_str(&self.product_id).map_err(boxed)?),
            picture: ProductPicture::image(self.image).map_err(boxed)?,
            kind: ProductImageKind::from_code(&self.kind)
                .ok_or_else(|| boxed(InvalidRow(format!("image kind {:?}", self.kind))))?,
            confirmed_at: from_unix_millis(self.confirmed_at),
        })
    }
}

impl ProductRepository for Database {
    fn list(&self, owner: ProfileId) -> Result<Vec<Product>, RepositoryError> {
        let conn = self.conn();
        let rows = conn
            .prepare(&format!(
                "{SELECT_PRODUCT} WHERE profile_id = ?1 ORDER BY name COLLATE NOCASE, rowid"
            ))
            .and_then(|mut statement| {
                statement
                    .query_map([owner.to_string()], ProductRow::read)?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(boxed)?;
        rows.into_iter().map(ProductRow::into_product).collect()
    }

    fn get(&self, id: ProductId) -> Result<Option<Product>, RepositoryError> {
        let row = self
            .conn()
            .query_row(
                &format!("{SELECT_PRODUCT} WHERE id = ?1"),
                [id.to_string()],
                ProductRow::read,
            )
            .optional()
            .map_err(boxed)?;
        row.map(ProductRow::into_product).transpose()
    }

    fn save(&self, product: &Product) -> Result<(), RepositoryError> {
        let brief = &product.brief;
        self.conn()
            .execute(
                "INSERT INTO product (id, profile_id, name, kind, description, audience, facts,
                                      offer, link, call_to_action, ownership, must_say, never_say,
                                      archived)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                 ON CONFLICT (id) DO UPDATE SET
                     name = excluded.name,
                     kind = excluded.kind,
                     description = excluded.description,
                     audience = excluded.audience,
                     facts = excluded.facts,
                     offer = excluded.offer,
                     link = excluded.link,
                     call_to_action = excluded.call_to_action,
                     ownership = excluded.ownership,
                     must_say = excluded.must_say,
                     never_say = excluded.never_say,
                     archived = excluded.archived,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
                params![
                    product.id.to_string(),
                    product.owner.to_string(),
                    brief.name(),
                    brief.kind().code(),
                    brief.description(),
                    brief.audience(),
                    brief.facts().join("\n"),
                    brief.offer(),
                    brief.link(),
                    brief.call_to_action(),
                    brief.ownership().code(),
                    brief.must_say(),
                    brief.never_say(),
                    product.archived,
                ],
            )
            .map_err(boxed)?;
        Ok(())
    }

    fn delete(&self, id: ProductId) -> Result<(), RepositoryError> {
        self.conn()
            .execute("DELETE FROM product WHERE id = ?1", [id.to_string()])
            .map_err(boxed)?;
        Ok(())
    }

    fn projects_using(&self, _id: ProductId) -> Result<usize, RepositoryError> {
        // No video project names a product yet: ad projects, which start
        // from a product's ad angle, bring the reference this counts.
        Ok(0)
    }

    fn images(&self, product: ProductId) -> Result<Vec<ProductImage>, RepositoryError> {
        let conn = self.conn();
        let rows = conn
            .prepare(
                "SELECT id, product_id, kind, image, confirmed_at FROM product_image
                 WHERE product_id = ?1 ORDER BY position",
            )
            .and_then(|mut statement| {
                statement
                    .query_map([product.to_string()], ImageRow::read)?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(boxed)?;
        rows.into_iter().map(ImageRow::into_image).collect()
    }

    fn add_image(&self, image: &ProductImage) -> Result<bool, RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        let product = image.product.to_string();
        let taken: Vec<i64> = tx
            .prepare("SELECT position FROM product_image WHERE product_id = ?1 ORDER BY position")
            .and_then(|mut statement| {
                statement
                    .query_map([&product], |row| row.get(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(boxed)?;
        if taken.len() >= MAX_PRODUCT_IMAGES {
            return Ok(false);
        }
        // Removing an image closes the gap, so the next place is the end.
        let position = taken.len() as i64;
        tx.execute(
            "INSERT INTO product_image (id, product_id, position, kind, image, format, width,
                                        height, confirmed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                image.id.to_string(),
                product,
                position,
                image.kind.code(),
                image.picture.bytes(),
                image.picture.format().mime(),
                image.picture.width(),
                image.picture.height(),
                to_unix_millis(image.confirmed_at),
            ],
        )
        .map_err(boxed)?;
        tx.commit().map_err(boxed)?;
        Ok(true)
    }

    fn set_image_kind(
        &self,
        product: ProductId,
        image: ProductImageId,
        kind: ProductImageKind,
    ) -> Result<bool, RepositoryError> {
        let changed = self
            .conn()
            .execute(
                "UPDATE product_image SET kind = ?3 WHERE id = ?2 AND product_id = ?1",
                params![product.to_string(), image.to_string(), kind.code()],
            )
            .map_err(boxed)?;
        Ok(changed > 0)
    }

    fn remove_image(
        &self,
        product: ProductId,
        image: ProductImageId,
    ) -> Result<bool, RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        let product = product.to_string();
        let position: Option<i64> = tx
            .query_row(
                "SELECT position FROM product_image WHERE id = ?2 AND product_id = ?1",
                params![product, image.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(boxed)?;
        let Some(position) = position else {
            return Ok(false);
        };
        tx.execute(
            "DELETE FROM product_image WHERE id = ?1",
            [image.to_string()],
        )
        .map_err(boxed)?;
        // The images after it move up one place, lowest first so no two
        // ever share one.
        let after: Vec<i64> = tx
            .prepare(
                "SELECT position FROM product_image
                 WHERE product_id = ?1 AND position > ?2 ORDER BY position",
            )
            .and_then(|mut statement| {
                statement
                    .query_map(params![product, position], |row| row.get(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(boxed)?;
        for position in after {
            tx.execute(
                "UPDATE product_image SET position = ?2 - 1
                 WHERE product_id = ?1 AND position = ?2",
                params![product, position],
            )
            .map_err(boxed)?;
        }
        tx.commit().map_err(boxed)?;
        Ok(true)
    }

    fn logo(&self, product: ProductId) -> Result<Option<ProductLogo>, RepositoryError> {
        let row: Option<(Vec<u8>, i64)> = self
            .conn()
            .query_row(
                "SELECT image, confirmed_at FROM product_logo WHERE product_id = ?1",
                [product.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(boxed)?;
        row.map(|(image, confirmed_at)| {
            Ok(ProductLogo {
                product,
                picture: ProductPicture::logo(image).map_err(boxed)?,
                confirmed_at: from_unix_millis(confirmed_at),
            })
        })
        .transpose()
    }

    fn save_logo(&self, logo: &ProductLogo) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "INSERT INTO product_logo (product_id, image, width, height, confirmed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (product_id) DO UPDATE SET
                     image = excluded.image,
                     width = excluded.width,
                     height = excluded.height,
                     confirmed_at = excluded.confirmed_at",
                params![
                    logo.product.to_string(),
                    logo.picture.bytes(),
                    logo.picture.width(),
                    logo.picture.height(),
                    to_unix_millis(logo.confirmed_at),
                ],
            )
            .map_err(boxed)?;
        Ok(())
    }

    fn remove_logo(&self, product: ProductId) -> Result<bool, RepositoryError> {
        let removed = self
            .conn()
            .execute(
                "DELETE FROM product_logo WHERE product_id = ?1",
                [product.to_string()],
            )
            .map_err(boxed)?;
        Ok(removed > 0)
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use bardo_domain::{ProfileRepository, UiLanguage, UserProfile};

    use super::*;

    fn database_with_profile() -> (Database, ProfileId) {
        let db = Database::open_in_memory().unwrap();
        let profile = UserProfile::new(UiLanguage::EnUs);
        ProfileRepository::save(&db, &profile).unwrap();
        (db, profile.id)
    }

    fn product(owner: ProfileId, name: &str) -> Product {
        let brief = ProductBrief::validate(ProductDraft {
            name: name.into(),
            kind: ProductKind::Physical,
            description: "A notebook whose pages are refilled.".into(),
            audience: "Students and people who draw.".into(),
            facts: "Refillable pages\nA5 size".into(),
            offer: "20% off until Friday".into(),
            link: "https://example.com/notebook".into(),
            call_to_action: "Get yours".into(),
            ownership: ProductOwnership::Client,
            must_say: "Made in Brazil".into(),
            never_say: "cheap".into(),
        })
        .unwrap();
        Product::new(owner, brief)
    }

    /// A PNG header of this size, RGBA, padded so two files differ.
    fn png(width: u32, height: u32, mark: u8) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        bytes.extend(width.to_be_bytes());
        bytes.extend(height.to_be_bytes());
        bytes.extend([8, 6, 0, 0, 0, mark]);
        bytes
    }

    fn at(seconds: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)
    }

    fn image(product: ProductId, mark: u8, kind: ProductImageKind) -> ProductImage {
        ProductImage::new(
            product,
            ProductPicture::image(png(800, 1000, mark)).unwrap(),
            kind,
            at(1_795_000_000 + u64::from(mark)),
        )
    }

    #[test]
    fn a_saved_product_is_read_back_with_every_field() {
        let (db, owner) = database_with_profile();
        let mut saved = product(owner, "Notebook");
        saved.archived = true;
        ProductRepository::save(&db, &saved).unwrap();
        assert_eq!(
            ProductRepository::get(&db, saved.id).unwrap(),
            Some(saved.clone())
        );
        assert_eq!(ProductRepository::list(&db, owner).unwrap(), [saved]);
    }

    #[test]
    fn saving_again_updates_in_place() {
        let (db, owner) = database_with_profile();
        let mut saved = product(owner, "Notebook");
        ProductRepository::save(&db, &saved).unwrap();
        saved.brief = ProductBrief::validate(ProductDraft {
            name: "Sketchbook".into(),
            kind: ProductKind::Idea,
            facts: String::new(),
            ..ProductDraft::from(&saved.brief)
        })
        .unwrap();
        ProductRepository::save(&db, &saved).unwrap();
        assert_eq!(ProductRepository::list(&db, owner).unwrap(), [saved]);
    }

    #[test]
    fn the_list_is_ordered_by_name_and_scoped_to_the_owner() {
        let (db, owner) = database_with_profile();
        let other = UserProfile::new(UiLanguage::EnUs);
        ProfileRepository::save(&db, &other).unwrap();
        for name in ["beta", "Alpha", "Gamma"] {
            ProductRepository::save(&db, &product(owner, name)).unwrap();
        }
        ProductRepository::save(&db, &product(other.id, "Theirs")).unwrap();
        let names: Vec<_> = ProductRepository::list(&db, owner)
            .unwrap()
            .into_iter()
            .map(|product| product.brief.name().to_owned())
            .collect();
        assert_eq!(names, ["Alpha", "beta", "Gamma"]);
    }

    #[test]
    fn two_products_of_one_profile_cannot_share_a_name() {
        let (db, owner) = database_with_profile();
        ProductRepository::save(&db, &product(owner, "Notebook")).unwrap();
        assert!(ProductRepository::save(&db, &product(owner, "NOTEBOOK")).is_err());
    }

    #[test]
    fn images_keep_their_file_kind_confirmation_and_order() {
        let (db, owner) = database_with_profile();
        let saved = product(owner, "Notebook");
        ProductRepository::save(&db, &saved).unwrap();
        let first = image(saved.id, 1, ProductImageKind::Photo);
        let second = image(saved.id, 2, ProductImageKind::Screen);
        assert!(db.add_image(&first).unwrap());
        assert!(db.add_image(&second).unwrap());
        assert_eq!(
            db.images(saved.id).unwrap(),
            [first.clone(), second.clone()]
        );

        assert!(
            db.set_image_kind(saved.id, first.id, ProductImageKind::Screen)
                .unwrap()
        );
        assert_eq!(
            db.images(saved.id).unwrap()[0].kind,
            ProductImageKind::Screen
        );
        // Not through another product, and not one that is gone.
        let other = product(owner, "Other");
        ProductRepository::save(&db, &other).unwrap();
        assert!(
            !db.set_image_kind(other.id, first.id, ProductImageKind::Photo)
                .unwrap()
        );
        assert!(!db.remove_image(other.id, first.id).unwrap());
        assert!(
            !db.set_image_kind(saved.id, ProductImageId::new(), ProductImageKind::Photo)
                .unwrap()
        );
    }

    #[test]
    fn a_fourth_image_is_not_added_and_a_removed_one_frees_its_place() {
        let (db, owner) = database_with_profile();
        let saved = product(owner, "Notebook");
        ProductRepository::save(&db, &saved).unwrap();
        let images: Vec<_> = (1..=4)
            .map(|mark| image(saved.id, mark, ProductImageKind::Photo))
            .collect();
        for image in &images[..3] {
            assert!(db.add_image(image).unwrap());
        }
        assert!(!db.add_image(&images[3]).unwrap());
        assert_eq!(db.images(saved.id).unwrap().len(), 3);

        // The first goes: the others move up and the new one comes last.
        assert!(db.remove_image(saved.id, images[0].id).unwrap());
        assert!(!db.remove_image(saved.id, images[0].id).unwrap());
        assert!(db.add_image(&images[3]).unwrap());
        let ids: Vec<_> = db
            .images(saved.id)
            .unwrap()
            .into_iter()
            .map(|image| image.id)
            .collect();
        assert_eq!(ids, [images[1].id, images[2].id, images[3].id]);
    }

    #[test]
    fn a_logo_is_one_per_product_and_replaced_in_place() {
        let (db, owner) = database_with_profile();
        let saved = product(owner, "Notebook");
        ProductRepository::save(&db, &saved).unwrap();
        assert_eq!(db.logo(saved.id).unwrap(), None);
        let logo = |mark: u8| ProductLogo {
            product: saved.id,
            picture: ProductPicture::logo(png(800, 200, mark)).unwrap(),
            confirmed_at: at(1_795_000_000 + u64::from(mark)),
        };
        db.save_logo(&logo(1)).unwrap();
        db.save_logo(&logo(2)).unwrap();
        assert_eq!(db.logo(saved.id).unwrap(), Some(logo(2)));
        assert!(db.remove_logo(saved.id).unwrap());
        assert!(!db.remove_logo(saved.id).unwrap());
        assert_eq!(db.logo(saved.id).unwrap(), None);
    }

    #[test]
    fn deleting_a_product_takes_its_images_and_logo() {
        let (db, owner) = database_with_profile();
        let saved = product(owner, "Notebook");
        let kept = product(owner, "Kept");
        ProductRepository::save(&db, &saved).unwrap();
        ProductRepository::save(&db, &kept).unwrap();
        db.add_image(&image(saved.id, 1, ProductImageKind::Photo))
            .unwrap();
        db.add_image(&image(kept.id, 2, ProductImageKind::Photo))
            .unwrap();
        db.save_logo(&ProductLogo {
            product: saved.id,
            picture: ProductPicture::logo(png(800, 200, 3)).unwrap(),
            confirmed_at: at(1_795_000_000),
        })
        .unwrap();
        assert_eq!(db.projects_using(saved.id).unwrap(), 0);

        ProductRepository::delete(&db, saved.id).unwrap();
        ProductRepository::delete(&db, saved.id).unwrap();
        assert_eq!(ProductRepository::get(&db, saved.id).unwrap(), None);
        let conn = db.conn();
        let count = |table: &str| -> i64 {
            conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap()
        };
        assert_eq!(count("product_image"), 1);
        assert_eq!(count("product_logo"), 0);
    }

    #[test]
    fn a_row_changed_outside_the_app_does_not_read_as_a_product() {
        let (db, owner) = database_with_profile();
        let saved = product(owner, "Notebook");
        ProductRepository::save(&db, &saved).unwrap();
        db.add_image(&image(saved.id, 1, ProductImageKind::Photo))
            .unwrap();
        db.conn()
            .execute("UPDATE product_image SET image = x'474946383961'", [])
            .unwrap();
        assert!(db.images(saved.id).is_err());
        db.conn()
            .execute("UPDATE product SET kind = 'gadget'", [])
            .unwrap();
        assert!(ProductRepository::get(&db, saved.id).is_err());
    }
}
