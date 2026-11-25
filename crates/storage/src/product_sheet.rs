//! A product's sheet (#274): its text, the claims an ad may make and the
//! pages a research read. Saved whole in one transaction; a claim keeps
//! its row for as long as it is on the sheet.

use bardo_domain::{
    Alternative, AudienceSegment, Claim, ClaimId, ClaimKind, ClaimRecord, ClaimSource, ClaimState,
    Objection, ProductId, ProductSheet, RepositoryError, RestrictedCategory, SheetSource,
    SheetText,
};
use rusqlite::{Connection, OptionalExtension, Row, params};
use uuid::Uuid;

use crate::{Database, boxed, from_unix_millis, to_unix_millis};

#[derive(Debug, thiserror::Error)]
#[error("stored product sheet is invalid: {0}")]
struct InvalidRow(String);

fn invalid(what: impl Into<String>) -> RepositoryError {
    boxed(InvalidRow(what.into()))
}

/// A claim row as stored, before domain checks.
struct ClaimRow {
    id: String,
    text: String,
    kind: String,
    source: [String; 3],
    state: String,
    flagged: bool,
    sensitive_choice: Option<bool>,
    confirmed_at: Option<i64>,
    edited_at: Option<i64>,
    created_at: i64,
}

impl ClaimRow {
    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            text: row.get(1)?,
            kind: row.get(2)?,
            source: [row.get(3)?, row.get(4)?, row.get(5)?],
            state: row.get(6)?,
            flagged: row.get(7)?,
            sensitive_choice: row.get(8)?,
            confirmed_at: row.get(9)?,
            edited_at: row.get(10)?,
            created_at: row.get(11)?,
        })
    }

    fn into_claim(self) -> Result<Claim, RepositoryError> {
        let [source, url, title] = &self.source;
        let record = ClaimRecord {
            id: ClaimId::from(Uuid::parse_str(&self.id).map_err(boxed)?),
            kind: ClaimKind::from_code(&self.kind)
                .ok_or_else(|| invalid(format!("claim kind {:?}", self.kind)))?,
            source: ClaimSource::from_parts(source, url, title)
                .ok_or_else(|| invalid(format!("claim source {source:?}")))?,
            state: ClaimState::from_code(&self.state)
                .ok_or_else(|| invalid(format!("claim state {:?}", self.state)))?,
            flagged: self.flagged,
            sensitive_choice: self.sensitive_choice,
            confirmed_at: self.confirmed_at.map(from_unix_millis),
            edited_at: self.edited_at.map(from_unix_millis),
            created_at: from_unix_millis(self.created_at),
            text: self.text,
        };
        Claim::restore(record).ok_or_else(|| invalid("claim text"))
    }
}

/// The rows of one of the sheet's lists, in their order.
fn rows<T>(
    conn: &Connection,
    sql: &str,
    product: &str,
    read: impl Fn(&Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>, RepositoryError> {
    conn.prepare(sql)
        .and_then(|mut statement| {
            statement
                .query_map([product], read)?
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(boxed)
}

fn position(index: usize) -> i64 {
    i64::try_from(index).unwrap_or(i64::MAX)
}

impl Database {
    pub(crate) fn read_product_sheet(
        &self,
        product: ProductId,
    ) -> Result<Option<ProductSheet>, RepositoryError> {
        let conn = self.conn();
        let id = product.to_string();
        type Head = (
            String,
            String,
            Option<String>,
            bool,
            String,
            Option<i64>,
            i64,
        );
        let head: Option<Head> = conn
            .query_row(
                "SELECT summary, category, restricted, restricted_chosen, looks, researched_at,
                        updated_at
                 FROM product_sheet WHERE product_id = ?1",
                [&id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                    ))
                },
            )
            .optional()
            .map_err(boxed)?;
        let Some((
            summary,
            category,
            restricted,
            restricted_chosen,
            looks,
            researched_at,
            updated_at,
        )) = head
        else {
            return Ok(None);
        };
        let restricted = restricted
            .map(|code| {
                RestrictedCategory::from_code(&code)
                    .ok_or_else(|| invalid(format!("restricted category {code:?}")))
            })
            .transpose()?;
        let segments = rows(
            &conn,
            "SELECT name, wants, holds_back FROM product_sheet_segment
             WHERE product_id = ?1 ORDER BY position",
            &id,
            |row| {
                Ok(AudienceSegment {
                    name: row.get(0)?,
                    wants: row.get(1)?,
                    holds_back: row.get(2)?,
                })
            },
        )?;
        let objections = rows(
            &conn,
            "SELECT objection, answer FROM product_sheet_objection
             WHERE product_id = ?1 ORDER BY position",
            &id,
            |row| {
                Ok(Objection {
                    objection: row.get(0)?,
                    answer: row.get(1)?,
                })
            },
        )?;
        let alternatives = rows(
            &conn,
            "SELECT name, differs FROM product_sheet_alternative
             WHERE product_id = ?1 ORDER BY position",
            &id,
            |row| {
                Ok(Alternative {
                    name: row.get(0)?,
                    differs: row.get(1)?,
                })
            },
        )?;
        let sources = rows(
            &conn,
            "SELECT url, title, read_at FROM product_sheet_source
             WHERE product_id = ?1 ORDER BY position",
            &id,
            |row| {
                Ok(SheetSource {
                    url: row.get(0)?,
                    title: row.get(1)?,
                    read_at: from_unix_millis(row.get(2)?),
                })
            },
        )?;
        let claims = rows(
            &conn,
            "SELECT id, text, kind, source, source_url, source_title, state, flagged,
                    sensitive_choice, confirmed_at, edited_at, created_at
             FROM product_claim WHERE product_id = ?1 ORDER BY position, rowid",
            &id,
            ClaimRow::read,
        )?
        .into_iter()
        .map(ClaimRow::into_claim)
        .collect::<Result<Vec<_>, _>>()?;
        // Through the domain's checks, so text changed outside the app
        // cannot pass for a sheet within its limits.
        let text = SheetText {
            summary,
            category,
            restricted,
            segments,
            objections,
            alternatives,
            looks,
        }
        .validate()
        .map_err(|errors| invalid(format!("{errors:?}")))?;
        Ok(Some(ProductSheet::restore(
            product,
            text,
            claims,
            sources,
            restricted_chosen,
            researched_at.map(from_unix_millis),
            from_unix_millis(updated_at),
        )))
    }

    pub(crate) fn write_product_sheet(&self, sheet: &ProductSheet) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        let id = sheet.product.to_string();
        let text = sheet.text();
        tx.execute(
            "INSERT INTO product_sheet (product_id, summary, category, restricted,
                                        restricted_chosen, looks, researched_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (product_id) DO UPDATE SET
                 summary = excluded.summary,
                 category = excluded.category,
                 restricted = excluded.restricted,
                 restricted_chosen = excluded.restricted_chosen,
                 looks = excluded.looks,
                 researched_at = excluded.researched_at,
                 updated_at = excluded.updated_at",
            params![
                id,
                text.summary,
                text.category,
                text.restricted.map(RestrictedCategory::code),
                sheet.restricted_chosen(),
                text.looks,
                sheet.researched_at.map(to_unix_millis),
                to_unix_millis(sheet.updated_at),
            ],
        )
        .map_err(boxed)?;

        for table in [
            "product_sheet_segment",
            "product_sheet_objection",
            "product_sheet_alternative",
            "product_sheet_source",
        ] {
            tx.execute(&format!("DELETE FROM {table} WHERE product_id = ?1"), [&id])
                .map_err(boxed)?;
        }
        for (index, segment) in text.segments.iter().enumerate() {
            tx.execute(
                "INSERT INTO product_sheet_segment (product_id, position, name, wants, holds_back)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    id,
                    position(index),
                    segment.name,
                    segment.wants,
                    segment.holds_back
                ],
            )
            .map_err(boxed)?;
        }
        for (index, objection) in text.objections.iter().enumerate() {
            tx.execute(
                "INSERT INTO product_sheet_objection (product_id, position, objection, answer)
                 VALUES (?1, ?2, ?3, ?4)",
                params![id, position(index), objection.objection, objection.answer],
            )
            .map_err(boxed)?;
        }
        for (index, alternative) in text.alternatives.iter().enumerate() {
            tx.execute(
                "INSERT INTO product_sheet_alternative (product_id, position, name, differs)
                 VALUES (?1, ?2, ?3, ?4)",
                params![id, position(index), alternative.name, alternative.differs],
            )
            .map_err(boxed)?;
        }
        for (index, source) in sheet.sources().iter().enumerate() {
            tx.execute(
                "INSERT INTO product_sheet_source (product_id, position, url, title, read_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    id,
                    position(index),
                    source.url,
                    source.title,
                    to_unix_millis(source.read_at)
                ],
            )
            .map_err(boxed)?;
        }

        // A claim no longer on the sheet goes; the others keep their rows.
        let kept: Vec<String> = sheet
            .claims()
            .iter()
            .map(|claim| claim.id.to_string())
            .collect();
        let stored = rows(
            &tx,
            "SELECT id FROM product_claim WHERE product_id = ?1",
            &id,
            |row| row.get::<_, String>(0),
        )?;
        for gone in stored.iter().filter(|stored| !kept.contains(stored)) {
            tx.execute("DELETE FROM product_claim WHERE id = ?1", [gone])
                .map_err(boxed)?;
        }
        for (index, claim) in sheet.claims().iter().enumerate() {
            let source = claim.source();
            tx.execute(
                "INSERT INTO product_claim (id, product_id, position, text, kind, source,
                                            source_url, source_title, state, flagged,
                                            sensitive_choice, confirmed_at, edited_at, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                 ON CONFLICT (id) DO UPDATE SET
                     position = excluded.position,
                     text = excluded.text,
                     kind = excluded.kind,
                     source = excluded.source,
                     source_url = excluded.source_url,
                     source_title = excluded.source_title,
                     state = excluded.state,
                     flagged = excluded.flagged,
                     sensitive_choice = excluded.sensitive_choice,
                     confirmed_at = excluded.confirmed_at,
                     edited_at = excluded.edited_at
                 WHERE product_id = excluded.product_id",
                params![
                    claim.id.to_string(),
                    id,
                    position(index),
                    claim.text(),
                    claim.kind.code(),
                    source.code(),
                    source.url().unwrap_or_default(),
                    source.title(),
                    claim.state().code(),
                    claim.record().flagged,
                    claim.record().sensitive_choice,
                    claim.confirmed_at().map(to_unix_millis),
                    claim.edited_at().map(to_unix_millis),
                    to_unix_millis(claim.created_at),
                ],
            )
            .map_err(boxed)?;
        }
        tx.commit().map_err(boxed)
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use bardo_domain::{
        PageRead, Product, ProductBrief, ProductDraft, ProductRepository, ProfileRepository,
        UiLanguage, UserProfile, WrittenClaim, WrittenSheet, WrittenSource,
    };

    use super::*;

    fn at(seconds: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000 + seconds)
    }

    fn database_with_product() -> (Database, ProductId) {
        let db = Database::open_in_memory().unwrap();
        let profile = UserProfile::new(UiLanguage::EnUs);
        ProfileRepository::save(&db, &profile).unwrap();
        let brief = ProductBrief::validate(ProductDraft {
            name: "Desk lamp".into(),
            description: "A lamp that lights a whole desk.".into(),
            link: "https://lamp.example".into(),
            ..ProductDraft::default()
        })
        .unwrap();
        let product = Product::new(profile.id, brief);
        ProductRepository::save(&db, &product).unwrap();
        (db, product.id)
    }

    fn text() -> SheetText {
        SheetText {
            summary: "A desk lamp with a wide, even light.".into(),
            category: "Home office lighting".into(),
            restricted: Some(RestrictedCategory::Health),
            segments: vec![
                AudienceSegment {
                    name: "Remote workers".into(),
                    wants: "Less eye strain".into(),
                    holds_back: "Price".into(),
                },
                AudienceSegment {
                    name: "Students".into(),
                    ..AudienceSegment::default()
                },
            ],
            objections: vec![Objection {
                objection: "It looks big.".into(),
                answer: "It folds flat.".into(),
            }],
            alternatives: vec![Alternative {
                name: "Clip-on lamps".into(),
                differs: "Light a corner only.".into(),
            }],
            looks: "Matte black arm on a round base.".into(),
        }
    }

    fn filled(product: ProductId) -> ProductSheet {
        let mut sheet = ProductSheet::empty(product, at(0));
        sheet.set_text(text(), at(1)).unwrap();
        sheet.take_facts(&["Made of steel".to_owned()], at(2));
        let pages = [
            PageRead {
                url: "https://lamp.example/".into(),
                title: "Desk lamp".into(),
                read_at: Some(at(3)),
            },
            PageRead {
                url: "https://reviews.example/lamps".into(),
                title: "Lamps compared".into(),
                read_at: None,
            },
        ];
        let written = WrittenSheet {
            text: text(),
            claims: vec![
                WrittenClaim {
                    text: "Lights a 160 cm desk".into(),
                    kind: ClaimKind::Feature,
                    source: WrittenSource::Page("https://lamp.example".into()),
                    sensitive: false,
                },
                WrittenClaim {
                    text: "Rated best for long work days".into(),
                    kind: ClaimKind::Proof,
                    source: WrittenSource::Page("https://reviews.example/lamps".into()),
                    sensitive: true,
                },
                WrittenClaim {
                    text: "Folds flat".into(),
                    kind: ClaimKind::Feature,
                    source: WrittenSource::Brief,
                    sensitive: false,
                },
            ],
        };
        sheet.take_written(written, &pages, "https://lamp.example", at(4));
        sheet
    }

    #[test]
    fn a_product_without_a_sheet_has_none() {
        let (db, product) = database_with_product();
        assert_eq!(db.sheet(product).unwrap(), None);
    }

    #[test]
    fn a_saved_sheet_is_read_back_whole() {
        let (db, product) = database_with_product();
        let mut sheet = filled(product);
        assert_eq!(sheet.claims().len(), 4);
        assert_eq!(sheet.sources().len(), 2);
        let found = sheet.claims()[1].id;
        sheet
            .change_claim(found, at(5), |claim| {
                claim.set_sensitive(true);
                claim.confirm(at(5))
            })
            .unwrap();
        let rejected = sheet.claims()[2].id;
        sheet
            .change_claim(rejected, at(6), |claim| {
                claim.edit("Rated best for long days", at(6))?;
                claim.reject()
            })
            .unwrap();

        db.save_sheet(&sheet).unwrap();

        assert_eq!(db.sheet(product).unwrap(), Some(sheet));
    }

    #[test]
    fn saving_again_replaces_the_lists_and_keeps_each_claims_row() {
        let (db, product) = database_with_product();
        let mut sheet = filled(product);
        db.save_sheet(&sheet).unwrap();
        let row_of = |id: ClaimId| -> Option<i64> {
            db.conn()
                .query_row(
                    "SELECT rowid FROM product_claim WHERE id = ?1",
                    [id.to_string()],
                    |row| row.get(0),
                )
                .optional()
                .unwrap()
        };
        let kept = sheet.claims()[0].id;
        let kept_row = row_of(kept);
        assert!(kept_row.is_some());

        let mut shorter = text();
        shorter.segments.truncate(1);
        shorter.objections.clear();
        shorter.restricted = None;
        sheet.set_text(shorter, at(10)).unwrap();
        sheet.take_facts(&["Five-year warranty".to_owned()], at(11));
        db.save_sheet(&sheet).unwrap();

        let stored = db.sheet(product).unwrap().unwrap();
        assert_eq!(stored, sheet);
        assert!(stored.restricted_chosen(), "the user took the category out");
        assert_eq!(stored.text().segments.len(), 1);
        assert!(stored.text().objections.is_empty());
        assert_eq!(row_of(kept), kept_row, "the claim's row is the same one");

        // A claim that left the sheet leaves the table.
        let fewer = ProductSheet::restore(
            product,
            sheet.text().clone(),
            sheet.claims()[..1].to_vec(),
            Vec::new(),
            sheet.restricted_chosen(),
            None,
            at(12),
        );
        db.save_sheet(&fewer).unwrap();
        assert_eq!(db.sheet(product).unwrap(), Some(fewer));
        let claims: i64 = db
            .conn()
            .query_row("SELECT count(*) FROM product_claim", [], |row| row.get(0))
            .unwrap();
        assert_eq!(claims, 1);
    }

    #[test]
    fn deleting_a_product_takes_its_sheet() {
        let (db, product) = database_with_product();
        db.save_sheet(&filled(product)).unwrap();
        db.delete(product).unwrap();
        for table in [
            "product_sheet",
            "product_sheet_segment",
            "product_sheet_objection",
            "product_sheet_alternative",
            "product_sheet_source",
            "product_claim",
        ] {
            let left: i64 = db
                .conn()
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(left, 0, "{table}");
        }
    }

    #[test]
    fn a_row_changed_outside_the_app_does_not_read_as_a_sheet() {
        let (db, product) = database_with_product();
        db.save_sheet(&filled(product)).unwrap();
        db.conn()
            .execute("UPDATE product_claim SET state = 'approved'", [])
            .unwrap();
        assert!(db.sheet(product).is_err());
    }
}
