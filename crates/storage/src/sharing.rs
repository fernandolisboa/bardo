//! Sharing library items between projects (issue #340, ADR-0047 item 5).
//!
//! A linked copy is kept equal to its original by triggers, so every
//! change of the original, whoever makes it (the user, or a job drawing a
//! look or researching a product), reaches the copy's rows in the same
//! transaction, and a failing write rolls both back. The triggers are
//! derived from the tables' columns as they are, and written again on every
//! start: a column a later migration adds is followed without another step.

use std::time::SystemTime;

use bardo_domain::{
    CopyKind, Following, LibraryItem, LibraryKind, PersonaId, ProductId, ProfileId, ProjectId,
    RepositoryError, ShareableItem, Sharing, SharingRepository, TemplateKind, WorldItemId,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use uuid::Uuid;

use crate::{Database, boxed, to_unix_millis};

/// A fresh UUID, made by SQLite for each row a trigger copies.
const NEW_ID: &str = "lower(hex(randomblob(4)) || '-' || hex(randomblob(2)) || '-4' \
     || substr(hex(randomblob(2)), 2) || '-' || hex(randomblob(2)) || '-' || hex(randomblob(6)))";

/// Columns of an item's own row that a copy keeps as its own.
const OWN: [&str; 9] = [
    "id",
    "profile_id",
    "owner_project_id",
    "name",
    "created_at",
    "shareable",
    "linked_from",
    "follows_name",
    "follows_project",
];

/// A table whose rows are library items.
struct ItemTable {
    table: &'static str,
    /// Its own columns beyond `OWN` that a copy keeps.
    own: &'static [&'static str],
}

const PERSONA: ItemTable = ItemTable {
    table: "persona",
    own: &[],
};
const PRODUCT: ItemTable = ItemTable {
    table: "product",
    own: &["archived"],
};
const CHARACTER: ItemTable = ItemTable {
    table: "character",
    own: &["archived_at", "belongs_to", "kind"],
};

/// How a row of an item's table is matched to the copy's row.
enum Key {
    /// By its own id: the copy's row names it in `copied_from`.
    Id,
    /// By these columns beside the item (none: one row per item).
    Columns(&'static [&'static str]),
}

/// A table holding what an item has: its looks, images, sheet, pictures.
struct Part {
    table: &'static str,
    item: &'static ItemTable,
    /// The column naming the item.
    parent: &'static str,
    key: Key,
    /// Which rows a copy carries, `{r}` standing for the row: a drawing
    /// waiting for review stays with the original.
    filter: Option<&'static str>,
}

/// Every part, parents before the parts that refer to them.
const PARTS: [Part; 13] = [
    Part {
        table: "persona_look",
        item: &PERSONA,
        parent: "persona_id",
        key: Key::Columns(&["slot"]),
        filter: Some("{r}.slot = 'current'"),
    },
    Part {
        table: "persona_look_asset",
        item: &PERSONA,
        parent: "persona_id",
        key: Key::Columns(&["provider"]),
        filter: None,
    },
    Part {
        table: "persona_look_face",
        item: &PERSONA,
        parent: "persona_id",
        key: Key::Columns(&[]),
        filter: None,
    },
    Part {
        table: "product_image",
        item: &PRODUCT,
        parent: "product_id",
        key: Key::Id,
        filter: None,
    },
    Part {
        table: "product_logo",
        item: &PRODUCT,
        parent: "product_id",
        key: Key::Columns(&[]),
        filter: None,
    },
    Part {
        table: "product_sheet",
        item: &PRODUCT,
        parent: "product_id",
        key: Key::Columns(&[]),
        filter: None,
    },
    Part {
        table: "product_sheet_segment",
        item: &PRODUCT,
        parent: "product_id",
        key: Key::Columns(&["position"]),
        filter: None,
    },
    Part {
        table: "product_sheet_objection",
        item: &PRODUCT,
        parent: "product_id",
        key: Key::Columns(&["position"]),
        filter: None,
    },
    Part {
        table: "product_sheet_alternative",
        item: &PRODUCT,
        parent: "product_id",
        key: Key::Columns(&["position"]),
        filter: None,
    },
    Part {
        table: "product_sheet_source",
        item: &PRODUCT,
        parent: "product_id",
        key: Key::Columns(&["position"]),
        filter: None,
    },
    Part {
        table: "product_claim",
        item: &PRODUCT,
        parent: "product_id",
        key: Key::Id,
        filter: None,
    },
    Part {
        table: "product_concept",
        item: &PRODUCT,
        parent: "product_id",
        key: Key::Id,
        filter: Some("{r}.slot = 'kept'"),
    },
    Part {
        table: "reference_picture",
        item: &CHARACTER,
        parent: "character_id",
        key: Key::Id,
        filter: Some("{r}.kept = 1"),
    },
];

fn parts() -> impl Iterator<Item = &'static Part> {
    PARTS.iter()
}

const TRIGGER_PREFIX: &str = "bardo_mirror_";

fn columns(conn: &Connection, table: &str) -> rusqlite::Result<Vec<String>> {
    let mut statement = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(names)
}

impl Part {
    /// The columns a copy's row takes from the original's.
    fn data(&self, conn: &Connection) -> rusqlite::Result<Vec<String>> {
        Ok(columns(conn, self.table)?
            .into_iter()
            .filter(|column| column != self.parent)
            .filter(|column| {
                !matches!(self.key, Key::Id) || (column != "id" && column != "copied_from")
            })
            .collect())
    }

    /// Matches a copy's row (`row`) to the original's (`of`: `OLD`/`NEW`).
    fn matches(&self, row: &str, of: &str) -> String {
        match self.key {
            Key::Id => format!("{row}.copied_from = {of}.id"),
            Key::Columns(keys) => keys
                .iter()
                .map(|key| format!("{row}.{key} = {of}.{key}"))
                .chain(std::iter::once("1".to_owned()))
                .collect::<Vec<_>>()
                .join(" AND "),
        }
    }

    fn filter_on(&self, row: &str) -> String {
        self.filter
            .map_or_else(|| "1".to_owned(), |filter| filter.replace("{r}", row))
    }

    /// The copies of the item that `of`'s row belongs to.
    fn copies(&self, of: &str) -> String {
        format!(
            "SELECT c.id FROM {item} c WHERE c.linked_from = {of}.{parent}",
            item = self.item.table,
            parent = self.parent
        )
    }

    /// Inserts `source`'s rows (an expression list over alias `r`) into
    /// each item `c` of `from`.
    fn insert_columns(&self, data: &[String]) -> String {
        let mut names = vec![self.parent.to_owned()];
        names.extend(data.iter().cloned());
        if matches!(self.key, Key::Id) {
            names.push("id".into());
            names.push("copied_from".into());
        }
        names.join(", ")
    }

    fn insert_values(&self, data: &[String], row: &str) -> String {
        let mut values = vec!["c.id".to_owned()];
        values.extend(data.iter().map(|column| format!("{row}.{column}")));
        if matches!(self.key, Key::Id) {
            values.push(NEW_ID.to_owned());
            values.push(format!("{row}.id"));
        }
        values.join(", ")
    }

    fn triggers(&self, conn: &Connection) -> rusqlite::Result<String> {
        let data = self.data(conn)?;
        let (table, item, parent) = (self.table, self.item.table, self.parent);
        let insert = format!(
            "INSERT INTO {table} ({columns}) SELECT {values} FROM {item} c \
             WHERE c.linked_from = NEW.{parent}",
            columns = self.insert_columns(&data),
            values = self.insert_values(&data, "NEW"),
        );
        let any_copy = |of: &str| {
            format!("EXISTS (SELECT 1 FROM {item} c WHERE c.linked_from = {of}.{parent})")
        };
        let set = data
            .iter()
            .map(|column| format!("{column} = NEW.{column}"))
            .collect::<Vec<_>>()
            .join(", ");
        let mut sql = String::new();
        sql.push_str(&format!(
            "CREATE TRIGGER {TRIGGER_PREFIX}{table}_insert AFTER INSERT ON {table} \
             WHEN {filter} AND {any} BEGIN {insert}; END;\n",
            filter = self.filter_on("NEW"),
            any = any_copy("NEW"),
        ));
        // The original's own row is still there: deleting the original
        // itself makes its copies independent first, content intact.
        sql.push_str(&format!(
            "CREATE TRIGGER {TRIGGER_PREFIX}{table}_delete AFTER DELETE ON {table} \
             WHEN {any} AND EXISTS (SELECT 1 FROM {item} o WHERE o.id = OLD.{parent}) BEGIN \
             DELETE FROM {table} WHERE {parent} IN ({copies}) AND {matches}; END;\n",
            any = any_copy("OLD"),
            copies = self.copies("OLD"),
            matches = self.matches(table, "OLD"),
        ));
        let mut update = format!(
            "UPDATE {table} SET {set} WHERE {parent} IN ({copies}) AND {matches};",
            copies = self.copies("OLD"),
            matches = self.matches(table, "OLD"),
        );
        if self.filter.is_some() {
            update.push_str(&format!(
                " DELETE FROM {table} WHERE NOT ({filter}) AND {parent} IN ({copies}) \
                 AND {matches};",
                filter = self.filter_on("NEW"),
                copies = self.copies("NEW"),
                matches = self.matches(table, "NEW"),
            ));
        }
        update.push_str(&format!(
            " {insert} AND {filter} AND NOT EXISTS (SELECT 1 FROM {table} x \
             WHERE x.{parent} = c.id AND {matches});",
            filter = self.filter_on("NEW"),
            matches = self.matches("x", "NEW"),
        ));
        sql.push_str(&format!(
            "CREATE TRIGGER {TRIGGER_PREFIX}{table}_update AFTER UPDATE ON {table} \
             WHEN {any} BEGIN {update} END;\n",
            any = any_copy("NEW"),
        ));
        Ok(sql)
    }
}

impl ItemTable {
    /// The columns a copy takes from its original.
    fn data(&self, conn: &Connection) -> rusqlite::Result<Vec<String>> {
        Ok(columns(conn, self.table)?
            .into_iter()
            .filter(|column| {
                !OWN.contains(&column.as_str()) && !self.own.contains(&column.as_str())
            })
            .collect())
    }

    /// What a copy's `belongs_to` becomes: the global character it
    /// belongs to stays; a project's, the copy of it in the copy's own
    /// project, if there is one.
    fn belongs_to(owner: &str, of: &str) -> String {
        format!(
            "CASE WHEN {of}.belongs_to IS NULL THEN NULL \
             WHEN (SELECT g.owner_project_id FROM character g WHERE g.id = {of}.belongs_to) IS NULL \
             THEN {of}.belongs_to \
             ELSE (SELECT x.id FROM character x WHERE x.linked_from = {of}.belongs_to \
                   AND x.owner_project_id IS {owner} LIMIT 1) END"
        )
    }

    fn triggers(&self, conn: &Connection) -> rusqlite::Result<String> {
        let table = self.table;
        let mut set: Vec<String> = self
            .data(conn)?
            .iter()
            .map(|column| format!("{column} = NEW.{column}"))
            .collect();
        set.push("follows_name = NEW.name".into());
        if table == "character" {
            set.push(format!(
                "belongs_to = {}",
                Self::belongs_to(&format!("{table}.owner_project_id"), "NEW")
            ));
        }
        Ok(format!(
            "CREATE TRIGGER {TRIGGER_PREFIX}{table}_update AFTER UPDATE ON {table} \
             WHEN EXISTS (SELECT 1 FROM {table} c WHERE c.linked_from = NEW.id) BEGIN \
             UPDATE {table} SET {set} WHERE linked_from = NEW.id; END;\n\
             CREATE TRIGGER {TRIGGER_PREFIX}{table}_unlink BEFORE DELETE ON {table} \
             WHEN EXISTS (SELECT 1 FROM {table} c WHERE c.linked_from = OLD.id) BEGIN \
             UPDATE {table} SET linked_from = NULL WHERE linked_from = OLD.id; END;\n",
            set = set.join(", "),
        ))
    }
}

/// A linked template takes each new version of its original's, as its own
/// next version.
const TEMPLATE_TRIGGER: &str = "CREATE TRIGGER bardo_mirror_template_version_insert \
     AFTER INSERT ON template_version \
     WHEN NEW.owner_project_id IS NOT NULL AND NEW.retired_at IS NULL \
     AND EXISTS (SELECT 1 FROM template_share s \
                 WHERE s.linked_from = NEW.owner_project_id AND s.kind = NEW.kind) \
     BEGIN \
     INSERT INTO template_version \
         (id, profile_id, owner_project_id, kind, number, instructions, prompt, created_at) \
     SELECT {NEW_ID}, NEW.profile_id, s.owner_project_id, NEW.kind, \
         (SELECT IFNULL(MAX(v.number), 0) + 1 FROM template_version v \
          WHERE v.profile_id = NEW.profile_id AND v.owner_project_id = s.owner_project_id \
          AND v.kind = NEW.kind), \
         NEW.instructions, NEW.prompt, NEW.created_at \
     FROM template_share s WHERE s.linked_from = NEW.owner_project_id AND s.kind = NEW.kind; \
     END;";

/// Every trigger that keeps linked copies equal to their originals.
pub(crate) fn mirror_triggers(conn: &Connection) -> rusqlite::Result<String> {
    let mut sql = String::new();
    for item in [&PERSONA, &PRODUCT, &CHARACTER] {
        sql.push_str(&item.triggers(conn)?);
    }
    for part in parts() {
        sql.push_str(&part.triggers(conn)?);
    }
    sql.push_str(&TEMPLATE_TRIGGER.replace("{NEW_ID}", NEW_ID));
    Ok(sql)
}

/// Drops the triggers, so a migration may change the tables they read.
pub(crate) fn drop_mirrors(conn: &Connection) -> rusqlite::Result<()> {
    let old: Vec<String> = {
        let mut statement = conn.prepare(
            "SELECT name FROM sqlite_master WHERE type = 'trigger' AND name LIKE 'bardo_mirror_%'",
        )?;
        statement
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    for name in old {
        conn.execute_batch(&format!("DROP TRIGGER {name};"))?;
    }
    Ok(())
}

/// Writes the triggers again, as the tables are now, in one transaction:
/// another process writing meanwhile always finds them.
pub(crate) fn install_mirrors(conn: &mut Connection) -> rusqlite::Result<()> {
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    drop_mirrors(&tx)?;
    let sql = mirror_triggers(&tx)?;
    tx.execute_batch(&sql)?;
    tx.commit()
}

/// Takes the database back to before migration 85, for the tests that
/// rewind further to check an older migration.
#[cfg(test)]
pub(crate) fn undo_for_tests(conn: &Connection) {
    drop_mirrors(conn).unwrap();
    conn.execute_batch(
        "DROP TABLE template_share;
         ALTER TABLE template_version DROP COLUMN retired_at;
         ALTER TABLE reference_picture DROP COLUMN copied_from;
         ALTER TABLE product_concept DROP COLUMN copied_from;
         ALTER TABLE product_claim DROP COLUMN copied_from;
         ALTER TABLE product_image DROP COLUMN copied_from;
         DROP INDEX persona_linked_from; DROP INDEX product_linked_from;
         DROP INDEX character_linked_from;
         ALTER TABLE persona DROP COLUMN follows_project;
         ALTER TABLE persona DROP COLUMN follows_name;
         ALTER TABLE persona DROP COLUMN linked_from;
         ALTER TABLE persona DROP COLUMN shareable;
         ALTER TABLE product DROP COLUMN follows_project;
         ALTER TABLE product DROP COLUMN follows_name;
         ALTER TABLE product DROP COLUMN linked_from;
         ALTER TABLE product DROP COLUMN shareable;
         ALTER TABLE character DROP COLUMN follows_project;
         ALTER TABLE character DROP COLUMN follows_name;
         ALTER TABLE character DROP COLUMN linked_from;
         ALTER TABLE character DROP COLUMN shareable;",
    )
    .unwrap();
}

fn item_table(item: LibraryItem) -> Option<(&'static ItemTable, String)> {
    match item {
        LibraryItem::Narrator(id) => Some((&PERSONA, id.to_string())),
        LibraryItem::Product(id) => Some((&PRODUCT, id.to_string())),
        LibraryItem::World(id) => Some((&CHARACTER, id.to_string())),
        LibraryItem::Template { .. } => None,
    }
}

fn item_of(table: &ItemTable, id: &str) -> Result<LibraryItem, RepositoryError> {
    let id = Uuid::parse_str(id).map_err(boxed)?;
    Ok(match table.table {
        "persona" => LibraryItem::Narrator(PersonaId::from(id)),
        "product" => LibraryItem::Product(ProductId::from(id)),
        _ => LibraryItem::World(WorldItemId::from(id)),
    })
}

fn project_id(text: &str) -> Result<ProjectId, RepositoryError> {
    Uuid::parse_str(text).map(ProjectId::from).map_err(boxed)
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct SharingRefused(&'static str);

fn refused(why: &'static str) -> RepositoryError {
    boxed(SharingRefused(why))
}

/// A project's name.
fn project_name(tx: &Connection, project: &str) -> rusqlite::Result<String> {
    tx.query_row("SELECT name FROM project WHERE id = ?1", [project], |row| {
        row.get(0)
    })
    .optional()
    .map(Option::unwrap_or_default)
}

/// The latest version still in use of `project`'s (or the global)
/// template of `kind`: id, instructions, prompt.
fn latest_template(
    tx: &Connection,
    project: Option<&str>,
    kind: TemplateKind,
) -> rusqlite::Result<Option<(String, String, String, String)>> {
    tx.query_row(
        "SELECT profile_id, instructions, prompt, id FROM template_version
         WHERE owner_project_id IS ?1 AND kind = ?2 AND retired_at IS NULL
         ORDER BY number DESC LIMIT 1",
        params![project, kind.code()],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )
    .optional()
}

/// Adds a version of `owner`'s template of `kind`, after every number it
/// ever had.
fn add_template_version(
    tx: &Connection,
    (profile, project): (&str, Option<&str>),
    kind: TemplateKind,
    (instructions, prompt): (&str, &str),
    now: SystemTime,
) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO template_version
             (id, profile_id, owner_project_id, kind, number, instructions, prompt, created_at)
         VALUES (?1, ?2, ?3, ?4,
             (SELECT IFNULL(MAX(number), 0) + 1 FROM template_version
              WHERE profile_id = ?2 AND owner_project_id IS ?3 AND kind = ?4),
             ?5, ?6, ?7)",
        params![
            Uuid::new_v4().to_string(),
            profile,
            project,
            kind.code(),
            instructions,
            prompt,
            to_unix_millis(now),
        ],
    )?;
    Ok(())
}

/// Copies the item's own row and its parts into `into`.
fn copy_rows(
    tx: &Transaction<'_>,
    table: &ItemTable,
    (original, copy): (&str, &str),
    (into, name): (Option<&str>, &str),
    linked: bool,
    now: SystemTime,
) -> Result<(), RepositoryError> {
    let all = columns(tx, table.table).map_err(boxed)?;
    let values: Vec<String> = all
        .iter()
        .map(|column| match column.as_str() {
            "id" => "?1".to_owned(),
            "owner_project_id" => "?2".to_owned(),
            "name" => "?3".to_owned(),
            "shareable" => "0".to_owned(),
            "linked_from" => "CASE WHEN ?4 THEN o.id END".to_owned(),
            "follows_name" => "CASE WHEN ?4 THEN o.name END".to_owned(),
            "follows_project" => {
                "CASE WHEN ?4 THEN (SELECT p.name FROM project p WHERE p.id = o.owner_project_id) END"
                    .to_owned()
            }
            "archived" => "0".to_owned(),
            "archived_at" => "NULL".to_owned(),
            "created_at" if table.table == "character" => "?5".to_owned(),
            "created_at" | "updated_at" => "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')".to_owned(),
            "belongs_to" => ItemTable::belongs_to("?2", "o"),
            other => format!("o.{other}"),
        })
        .collect();
    tx.execute(
        &format!(
            "INSERT INTO {t} ({columns}) SELECT {values} FROM {t} o WHERE o.id = ?6",
            t = table.table,
            columns = all.join(", "),
            values = values.join(", "),
        ),
        params![copy, into, name, linked, to_unix_millis(now), original],
    )
    .map_err(boxed)?;
    for part in parts().filter(|part| std::ptr::eq(part.item, table)) {
        let data = part.data(tx).map_err(boxed)?;
        tx.execute(
            &format!(
                "INSERT INTO {t} ({columns}) SELECT {values} FROM {t} r, (SELECT ?1 AS id) c \
                 WHERE r.{parent} = ?2 AND {filter}",
                t = part.table,
                columns = part.insert_columns(&data),
                values = part.insert_values(&data, "r"),
                parent = part.parent,
                filter = part.filter_on("r"),
            ),
            params![copy, original],
        )
        .map_err(boxed)?;
    }
    Ok(())
}

impl Database {
    fn template_sharing(
        conn: &Connection,
        project: ProjectId,
        kind: TemplateKind,
    ) -> Result<Sharing, RepositoryError> {
        let row: Option<(bool, Option<String>, Option<String>, Option<String>)> = conn
            .query_row(
                "SELECT s.shareable, s.linked_from, s.follows_project,
                     (SELECT p.name FROM project p WHERE p.id = s.linked_from)
                 FROM template_share s WHERE s.owner_project_id = ?1 AND s.kind = ?2",
                params![project.to_string(), kind.code()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()
            .map_err(boxed)?;
        let Some((shareable, linked, follows, live)) = row else {
            return Ok(Sharing::default());
        };
        let following = follows
            .map(|follows| -> Result<Following, RepositoryError> {
                Ok(Following {
                    name: String::new(),
                    project: live.unwrap_or(follows),
                    original: linked
                        .as_deref()
                        .map(project_id)
                        .transpose()?
                        .map(|from| LibraryItem::template(from, kind)),
                })
            })
            .transpose()?;
        Ok(Sharing {
            shareable,
            following,
        })
    }

    fn item_sharing(
        conn: &Connection,
        table: &ItemTable,
        id: &str,
    ) -> Result<Sharing, RepositoryError> {
        let row: Option<(
            bool,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        )> = conn
            .query_row(
                &format!(
                    "SELECT t.shareable, t.linked_from, t.follows_name, t.follows_project,
                         (SELECT p.name FROM {t} o JOIN project p ON p.id = o.owner_project_id
                          WHERE o.id = t.linked_from)
                     FROM {t} t WHERE t.id = ?1",
                    t = table.table
                ),
                [id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()
            .map_err(boxed)?;
        let Some((shareable, linked, name, project, live)) = row else {
            return Ok(Sharing::default());
        };
        let following = name
            .map(|name| -> Result<Following, RepositoryError> {
                Ok(Following {
                    name,
                    project: live.or(project).unwrap_or_default(),
                    original: linked.as_deref().map(|id| item_of(table, id)).transpose()?,
                })
            })
            .transpose()?;
        Ok(Sharing {
            shareable,
            following,
        })
    }
}

impl SharingRepository for Database {
    fn sharing(&self, item: LibraryItem) -> Result<Sharing, RepositoryError> {
        let conn = self.conn();
        match item {
            LibraryItem::Template {
                project: Some(project),
                kind,
            } => Self::template_sharing(&conn, project, kind),
            LibraryItem::Template { project: None, .. } => Ok(Sharing::default()),
            item => {
                let (table, id) = item_table(item).expect("an item with a table");
                Self::item_sharing(&conn, table, &id)
            }
        }
    }

    fn sharing_of(
        &self,
        owner: ProfileId,
        project: Option<ProjectId>,
        kind: LibraryKind,
    ) -> Result<Vec<(LibraryItem, Sharing)>, RepositoryError> {
        let conn = self.conn();
        let project = project.map(|id| id.to_string());
        let (table, world) = match kind {
            LibraryKind::Template => {
                let Some(project) = project else {
                    return Ok(Vec::new());
                };
                let kinds: Vec<String> = conn
                    .prepare("SELECT kind FROM template_share WHERE owner_project_id = ?1")
                    .and_then(|mut statement| {
                        statement
                            .query_map([&project], |row| row.get(0))?
                            .collect::<rusqlite::Result<_>>()
                    })
                    .map_err(boxed)?;
                let project = project_id(&project)?;
                let mut found = Vec::new();
                for kind in kinds {
                    let Ok(kind) = kind.parse::<TemplateKind>() else {
                        continue;
                    };
                    let sharing = Self::template_sharing(&conn, project, kind)?;
                    if sharing.shareable || sharing.following.is_some() {
                        found.push((LibraryItem::template(project, kind), sharing));
                    }
                }
                return Ok(found);
            }
            LibraryKind::Narrator => (&PERSONA, None),
            LibraryKind::Product => (&PRODUCT, None),
            LibraryKind::World(kind) => (&CHARACTER, Some(kind.code())),
        };
        let ids: Vec<String> = conn
            .prepare(&format!(
                "SELECT id FROM {t} WHERE profile_id = ?1 AND owner_project_id IS ?2
                     AND (shareable = 1 OR follows_name IS NOT NULL) {world}",
                t = table.table,
                world = if world.is_some() {
                    "AND kind = ?3"
                } else {
                    "AND ?3 IS NULL"
                },
            ))
            .and_then(|mut statement| {
                statement
                    .query_map(params![owner.to_string(), project, world], |row| row.get(0))?
                    .collect::<rusqlite::Result<_>>()
            })
            .map_err(boxed)?;
        ids.iter()
            .map(|id| Ok((item_of(table, id)?, Self::item_sharing(&conn, table, id)?)))
            .collect()
    }

    fn set_shareable(&self, item: LibraryItem, shareable: bool) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        match item {
            LibraryItem::Template {
                project: Some(project),
                kind,
            } => {
                let (project, kind) = (project.to_string(), kind.code());
                let linked: bool = tx
                    .query_row(
                        "SELECT EXISTS (SELECT 1 FROM template_share WHERE owner_project_id = ?1
                             AND kind = ?2 AND linked_from IS NOT NULL)",
                        params![project, kind],
                        |row| row.get(0),
                    )
                    .map_err(boxed)?;
                if linked && shareable {
                    return Err(refused("a linked copy is not shareable"));
                }
                tx.execute(
                    "INSERT INTO template_share (owner_project_id, kind, shareable)
                     VALUES (?1, ?2, ?3)
                     ON CONFLICT (owner_project_id, kind) DO UPDATE SET shareable = excluded.shareable",
                    params![project, kind, shareable],
                )
                .map_err(boxed)?;
                if !shareable {
                    tx.execute(
                        "UPDATE template_share SET linked_from = NULL
                         WHERE linked_from = ?1 AND kind = ?2",
                        params![project, kind],
                    )
                    .map_err(boxed)?;
                }
            }
            LibraryItem::Template { project: None, .. } => {
                return Err(refused("the global library is not shared"));
            }
            item => {
                let (table, id) = item_table(item).expect("an item with a table");
                let (global, linked): (bool, bool) = tx
                    .query_row(
                        &format!(
                            "SELECT owner_project_id IS NULL, linked_from IS NOT NULL
                             FROM {t} WHERE id = ?1",
                            t = table.table
                        ),
                        [&id],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .map_err(boxed)?;
                if shareable && (global || linked) {
                    return Err(refused("only a project's own item is shareable"));
                }
                tx.execute(
                    &format!(
                        "UPDATE {t} SET shareable = ?2 WHERE id = ?1",
                        t = table.table
                    ),
                    params![id, shareable],
                )
                .map_err(boxed)?;
                if !shareable {
                    // Each copy says once that it became independent.
                    tx.execute(
                        &format!(
                            "UPDATE {t} SET linked_from = NULL WHERE linked_from = ?1",
                            t = table.table
                        ),
                        [&id],
                    )
                    .map_err(boxed)?;
                }
            }
        }
        tx.commit().map_err(boxed)
    }

    fn shareable_items(
        &self,
        owner: ProfileId,
        kind: LibraryKind,
    ) -> Result<Vec<ShareableItem>, RepositoryError> {
        let conn = self.conn();
        let (table, world) = match kind {
            LibraryKind::Template => {
                let rows: Vec<(String, String, String, String)> = conn
                    .prepare(
                        "SELECT s.owner_project_id, s.kind, p.name, p.state
                         FROM template_share s JOIN project p ON p.id = s.owner_project_id
                         WHERE p.profile_id = ?1 AND s.shareable = 1
                             AND EXISTS (SELECT 1 FROM template_version v
                                         WHERE v.owner_project_id = s.owner_project_id
                                         AND v.kind = s.kind AND v.retired_at IS NULL)
                         ORDER BY p.name COLLATE NOCASE, s.kind",
                    )
                    .and_then(|mut statement| {
                        statement
                            .query_map([owner.to_string()], |row| {
                                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                            })?
                            .collect::<rusqlite::Result<_>>()
                    })
                    .map_err(boxed)?;
                let mut items = Vec::new();
                for (project, kind, name, state) in rows {
                    let Ok(kind) = kind.parse::<TemplateKind>() else {
                        continue;
                    };
                    let project = project_id(&project)?;
                    items.push(ShareableItem {
                        item: LibraryItem::template(project, kind),
                        name: String::new(),
                        project,
                        project_name: name,
                        archived_project: state == "archived",
                    });
                }
                return Ok(items);
            }
            LibraryKind::Narrator => (&PERSONA, None),
            LibraryKind::Product => (&PRODUCT, None),
            LibraryKind::World(kind) => (&CHARACTER, Some(kind.code())),
        };
        let rows: Vec<(String, String, String, String, String)> = conn
            .prepare(&format!(
                "SELECT t.id, t.name, t.owner_project_id, p.name, p.state
                 FROM {t} t JOIN project p ON p.id = t.owner_project_id
                 WHERE t.profile_id = ?1 AND t.shareable = 1 {world}
                 ORDER BY p.name COLLATE NOCASE, t.name COLLATE NOCASE",
                t = table.table,
                world = if world.is_some() {
                    "AND t.kind = ?2"
                } else {
                    "AND ?2 IS NULL"
                },
            ))
            .and_then(|mut statement| {
                statement
                    .query_map(params![owner.to_string(), world], |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                        ))
                    })?
                    .collect::<rusqlite::Result<_>>()
            })
            .map_err(boxed)?;
        rows.into_iter()
            .map(|(id, name, project, project_name, state)| {
                Ok(ShareableItem {
                    item: item_of(table, &id)?,
                    name,
                    project: project_id(&project)?,
                    project_name,
                    archived_project: state == "archived",
                })
            })
            .collect()
    }

    fn copy_item(
        &self,
        item: LibraryItem,
        into: Option<ProjectId>,
        name: &str,
        copy: CopyKind,
        now: SystemTime,
    ) -> Result<LibraryItem, RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        let linked = copy == CopyKind::Linked;
        let into_text = into.map(|id| id.to_string());
        if linked && into.is_none() {
            return Err(refused("the global library holds no linked copy"));
        }
        let copied = match item {
            LibraryItem::Template { project, kind } => {
                let from = project.map(|id| id.to_string());
                let (profile, instructions, prompt, _) =
                    latest_template(&tx, from.as_deref(), kind)
                        .map_err(boxed)?
                        .ok_or_else(|| refused("the template has no version"))?;
                if linked {
                    let Some(from) = from.as_deref() else {
                        return Err(refused("the global template is used by reference"));
                    };
                    let into = into_text.as_deref().expect("linked into a project");
                    let follows = project_name(&tx, from).map_err(boxed)?;
                    tx.execute(
                        "INSERT INTO template_share
                             (owner_project_id, kind, shareable, linked_from, follows_project)
                         VALUES (?1, ?2, 0, ?3, ?4)
                         ON CONFLICT (owner_project_id, kind) DO UPDATE SET
                             shareable = 0, linked_from = excluded.linked_from,
                             follows_project = excluded.follows_project",
                        params![into, kind.code(), from, follows],
                    )
                    .map_err(boxed)?;
                }
                add_template_version(
                    &tx,
                    (&profile, into_text.as_deref()),
                    kind,
                    (&instructions, &prompt),
                    now,
                )
                .map_err(boxed)?;
                LibraryItem::Template {
                    project: into,
                    kind,
                }
            }
            item => {
                let (table, original) = item_table(item).expect("an item with a table");
                let linked_original: bool = tx
                    .query_row(
                        &format!(
                            "SELECT linked_from IS NOT NULL FROM {t} WHERE id = ?1",
                            t = table.table
                        ),
                        [&original],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(boxed)?
                    .ok_or_else(|| refused("the item is gone"))?;
                if linked && linked_original {
                    return Err(refused("a linked copy is not followed in turn"));
                }
                let id = Uuid::new_v4().to_string();
                copy_rows(
                    &tx,
                    table,
                    (&original, &id),
                    (into_text.as_deref(), name),
                    linked,
                    now,
                )?;
                item_of(table, &id)?
            }
        };
        tx.commit().map_err(boxed)?;
        Ok(copied)
    }

    fn linked_copies(&self, item: LibraryItem) -> Result<Vec<LibraryItem>, RepositoryError> {
        let conn = self.conn();
        match item {
            LibraryItem::Template {
                project: Some(project),
                kind,
            } => {
                let projects: Vec<String> = conn
                    .prepare(
                        "SELECT owner_project_id FROM template_share
                         WHERE linked_from = ?1 AND kind = ?2",
                    )
                    .and_then(|mut statement| {
                        statement
                            .query_map(params![project.to_string(), kind.code()], |row| row.get(0))?
                            .collect::<rusqlite::Result<_>>()
                    })
                    .map_err(boxed)?;
                projects
                    .iter()
                    .map(|id| Ok(LibraryItem::template(project_id(id)?, kind)))
                    .collect()
            }
            LibraryItem::Template { project: None, .. } => Ok(Vec::new()),
            item => {
                let (table, id) = item_table(item).expect("an item with a table");
                let ids: Vec<String> = conn
                    .prepare(&format!(
                        "SELECT id FROM {t} WHERE linked_from = ?1 ORDER BY id",
                        t = table.table
                    ))
                    .and_then(|mut statement| {
                        statement
                            .query_map([id], |row| row.get(0))?
                            .collect::<rusqlite::Result<_>>()
                    })
                    .map_err(boxed)?;
                ids.iter().map(|id| item_of(table, id)).collect()
            }
        }
    }

    fn unlink(&self, item: LibraryItem) -> Result<(), RepositoryError> {
        let conn = self.conn();
        match item {
            LibraryItem::Template {
                project: Some(project),
                kind,
            } => conn.execute(
                "UPDATE template_share SET linked_from = NULL, follows_project = NULL
                 WHERE owner_project_id = ?1 AND kind = ?2",
                params![project.to_string(), kind.code()],
            ),
            LibraryItem::Template { project: None, .. } => return Ok(()),
            item => {
                let (table, id) = item_table(item).expect("an item with a table");
                conn.execute(
                    &format!(
                        "UPDATE {t} SET linked_from = NULL, follows_name = NULL,
                             follows_project = NULL WHERE id = ?1",
                        t = table.table
                    ),
                    [id],
                )
            }
        }
        .map_err(boxed)?;
        Ok(())
    }

    fn forget_following(&self, item: LibraryItem) -> Result<(), RepositoryError> {
        let conn = self.conn();
        match item {
            LibraryItem::Template {
                project: Some(project),
                kind,
            } => conn.execute(
                "UPDATE template_share SET follows_project = NULL
                 WHERE owner_project_id = ?1 AND kind = ?2 AND linked_from IS NULL",
                params![project.to_string(), kind.code()],
            ),
            LibraryItem::Template { project: None, .. } => return Ok(()),
            item => {
                let (table, id) = item_table(item).expect("an item with a table");
                conn.execute(
                    &format!(
                        "UPDATE {t} SET follows_name = NULL, follows_project = NULL
                         WHERE id = ?1 AND linked_from IS NULL",
                        t = table.table
                    ),
                    [id],
                )
            }
        }
        .map_err(boxed)?;
        Ok(())
    }

    fn make_global(
        &self,
        item: LibraryItem,
        name: &str,
        now: SystemTime,
    ) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        match item {
            LibraryItem::Template {
                project: Some(project),
                kind,
            } => {
                let project = project.to_string();
                let (profile, instructions, prompt, _) = latest_template(&tx, Some(&project), kind)
                    .map_err(boxed)?
                    .ok_or_else(|| refused("the project has no template of its own"))?;
                add_template_version(&tx, (&profile, None), kind, (&instructions, &prompt), now)
                    .map_err(boxed)?;
                // The project and every project following it use the
                // global template again; generations keep their versions.
                tx.execute(
                    "UPDATE template_version SET retired_at = ?3
                     WHERE kind = ?2 AND retired_at IS NULL AND (owner_project_id = ?1
                         OR owner_project_id IN (SELECT owner_project_id FROM template_share
                                                 WHERE linked_from = ?1 AND kind = ?2))",
                    params![project, kind.code(), to_unix_millis(now)],
                )
                .map_err(boxed)?;
                tx.execute(
                    "DELETE FROM template_share
                     WHERE kind = ?2 AND (owner_project_id = ?1 OR linked_from = ?1)",
                    params![project, kind.code()],
                )
                .map_err(boxed)?;
            }
            LibraryItem::Template { project: None, .. } => {
                return Err(refused("the template is global already"));
            }
            item => {
                let (table, id) = item_table(item).expect("an item with a table");
                let copies: Vec<String> = tx
                    .prepare(&format!(
                        "SELECT id FROM {t} WHERE linked_from = ?1",
                        t = table.table
                    ))
                    .and_then(|mut statement| {
                        statement
                            .query_map([&id], |row| row.get(0))?
                            .collect::<rusqlite::Result<_>>()
                    })
                    .map_err(boxed)?;
                for copy in &copies {
                    repoint_uses(&tx, table, copy, &id)?;
                    tx.execute(
                        &format!("DELETE FROM {t} WHERE id = ?1", t = table.table),
                        [copy],
                    )
                    .map_err(boxed)?;
                }
                tx.execute(
                    &format!(
                        "UPDATE {t} SET owner_project_id = NULL, name = ?2, shareable = 0,
                             linked_from = NULL, follows_name = NULL, follows_project = NULL
                         WHERE id = ?1",
                        t = table.table
                    ),
                    params![id, name],
                )
                .map_err(boxed)?;
            }
        }
        tx.commit().map_err(boxed)
    }
}

/// Points every use of the linked copy `copy` at its original `to`, row
/// for row, before the copy is dropped.
fn repoint_uses(
    tx: &Transaction<'_>,
    table: &ItemTable,
    copy: &str,
    to: &str,
) -> Result<(), RepositoryError> {
    let statements: &[&str] = match table.table {
        "persona" => &[
            "UPDATE channel SET default_persona_id = ?2 WHERE default_persona_id = ?1",
            "UPDATE video_project SET persona_id = ?2 WHERE persona_id = ?1",
        ],
        "product" => &[
            "UPDATE video_project SET product_id = ?2 WHERE product_id = ?1",
            "UPDATE ad_angle SET product_id = ?2 WHERE product_id = ?1",
            "UPDATE ad_brief SET product_id = ?2 WHERE product_id = ?1",
            "UPDATE scene SET product_image_id = (SELECT i.copied_from FROM product_image i
                 WHERE i.id = scene.product_image_id)
             WHERE product_image_id IN (SELECT id FROM product_image WHERE product_id = ?1)",
            "UPDATE ad_angle_claim SET claim_id = (SELECT c.copied_from FROM product_claim c
                 WHERE c.id = ad_angle_claim.claim_id)
             WHERE claim_id IN (SELECT id FROM product_claim WHERE product_id = ?1
                                AND copied_from IS NOT NULL)",
            "UPDATE ad_brief_claim SET claim_id = (SELECT c.copied_from FROM product_claim c
                 WHERE c.id = ad_brief_claim.claim_id)
             WHERE claim_id IN (SELECT id FROM product_claim WHERE product_id = ?1
                                AND copied_from IS NOT NULL)",
            "UPDATE claims_check_statement SET claim_id = (SELECT c.copied_from
                 FROM product_claim c WHERE c.id = claims_check_statement.claim_id)
             WHERE claim_id IN (SELECT id FROM product_claim WHERE product_id = ?1
                                AND copied_from IS NOT NULL)",
        ],
        _ => &[
            "UPDATE video_cast_pick SET character_id = ?2 WHERE character_id = ?1",
            "UPDATE scene_cast SET character_id = ?2 WHERE character_id = ?1",
            "UPDATE scene_place SET item_id = ?2 WHERE item_id = ?1",
            "UPDATE scene SET objects = replace(objects, ?1, ?2) WHERE instr(objects, ?1) > 0",
            "UPDATE character SET belongs_to = ?2 WHERE belongs_to = ?1",
        ],
    };
    for statement in statements {
        let needs_to = statement.contains("?2");
        if needs_to {
            tx.execute(statement, params![copy, to])
        } else {
            tx.execute(statement, params![copy])
        }
        .map_err(boxed)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bardo_domain::{
        Channel, ChannelDetails, ChannelDraft, ChannelRepository, GenerationPresets, LookImage,
        Persona, PersonaDetails, PersonaDraft, PersonaLook, PersonaRepository, PictureOrigin,
        PictureProvenance, Product, ProductBrief, ProductDraft, ProductImage, ProductImageKind,
        ProductKind, ProductOwnership, ProductPicture, ProductRepository, ProfileRepository,
        Provider, ReferencePicture, ReferencePictureId, TemplateBody, TemplateRepository,
        TemplateVersion, UiLanguage, UserProfile, VoiceRef, WorldItem, WorldItemDetails,
        WorldItemDraft, WorldKind, WorldRepository,
    };

    use super::*;

    struct Setup {
        db: Database,
        owner: ProfileId,
        /// The project that shares.
        space: ProjectId,
        /// The project that imports.
        sea: ProjectId,
    }

    fn setup() -> Setup {
        let db = Database::open_in_memory().unwrap();
        let profile = UserProfile::new(UiLanguage::EnUs);
        ProfileRepository::save(&db, &profile).unwrap();
        let space = crate::project::test_project(&db, profile.id);
        let sea = crate::project::test_project(&db, profile.id);
        Setup {
            db,
            owner: profile.id,
            space,
            sea,
        }
    }

    fn at(seconds: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)
    }

    fn narrator(s: &Setup, tone: &str) -> Persona {
        let details = PersonaDetails::validate(PersonaDraft {
            name: "Ana".into(),
            voice: Some(VoiceRef::elevenlabs("FrS6cKLB1wg4WYgPa9GW", "Wyatt").unwrap()),
            tone: tone.into(),
            script_style: "Short sentences.".into(),
            presets: GenerationPresets {
                stability: 60,
                similarity: 75,
                style: 10,
                speed: 95,
            },
            realistic_voice: false,
        })
        .unwrap();
        let mut persona = Persona::new(s.owner, details);
        persona.owner_project = Some(s.space);
        PersonaRepository::save(&s.db, &persona).unwrap();
        persona
    }

    fn png(width: u32, mark: u8) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        bytes.extend(width.to_be_bytes());
        bytes.extend(1248_u32.to_be_bytes());
        bytes.extend([8, 6, 0, 0, 0, mark]);
        bytes
    }

    fn look(persona: PersonaId, mark: u8) -> PersonaLook {
        PersonaLook::imported(
            persona,
            LookImage::check(png(832, mark)).unwrap(),
            at(10 + u64::from(mark)),
        )
    }

    fn tone(db: &Database, id: PersonaId) -> String {
        PersonaRepository::get(db, id)
            .unwrap()
            .unwrap()
            .details
            .tone()
            .to_owned()
    }

    fn linked_narrator(s: &Setup) -> (Persona, PersonaId) {
        let original = narrator(s, "Calm.");
        PersonaRepository::save_look(&s.db, &look(original.id, 1)).unwrap();
        s.db.set_shareable(LibraryItem::Narrator(original.id), true)
            .unwrap();
        let LibraryItem::Narrator(copy) =
            s.db.copy_item(
                LibraryItem::Narrator(original.id),
                Some(s.sea),
                "Ana",
                CopyKind::Linked,
                at(5),
            )
            .unwrap()
        else {
            panic!("a narrator");
        };
        (original, copy)
    }

    #[test]
    fn a_linked_copy_carries_everything_and_follows_each_save_of_its_original() {
        let s = setup();
        let (mut original, copy) = linked_narrator(&s);
        let copied = PersonaRepository::get(&s.db, copy).unwrap().unwrap();
        assert_eq!(copied.owner_project, Some(s.sea));
        assert_eq!(copied.details.tone(), "Calm.");
        assert_eq!(
            PersonaRepository::look(&s.db, copy).unwrap().unwrap().image,
            PersonaRepository::look(&s.db, original.id)
                .unwrap()
                .unwrap()
                .image
        );
        let sharing = s.db.sharing(LibraryItem::Narrator(copy)).unwrap();
        assert!(sharing.is_linked());
        assert!(!sharing.shareable);
        assert_eq!(sharing.following.as_ref().unwrap().name, "Ana");

        original.details = PersonaDetails::validate(PersonaDraft {
            tone: "Warm.".into(),
            ..PersonaDraft::from(&original.details)
        })
        .unwrap();
        PersonaRepository::save(&s.db, &original).unwrap();
        assert_eq!(tone(&s.db, copy), "Warm.");

        // A new look, and a drawing waiting for review that stays behind.
        PersonaRepository::save_look(&s.db, &look(original.id, 2)).unwrap();
        assert_eq!(
            PersonaRepository::look(&s.db, copy).unwrap().unwrap().image,
            look(original.id, 2).image
        );
        PersonaRepository::save_pending_look(&s.db, &look(original.id, 3)).unwrap();
        assert!(
            PersonaRepository::pending_look(&s.db, copy)
                .unwrap()
                .is_none()
        );
        PersonaRepository::keep_pending_look(&s.db, original.id).unwrap();
        assert_eq!(
            PersonaRepository::look(&s.db, copy).unwrap().unwrap().image,
            look(original.id, 3).image,
            "kept, it reaches the copy"
        );
        PersonaRepository::remove_look(&s.db, original.id).unwrap();
        assert!(PersonaRepository::look(&s.db, copy).unwrap().is_none());
    }

    #[test]
    fn a_failing_write_rolls_back_the_original_and_its_copy_together() {
        let s = setup();
        let (original, copy) = linked_narrator(&s);
        {
            let mut conn = s.db.conn();
            let tx = conn.transaction().unwrap();
            tx.execute(
                "UPDATE persona SET tone = 'Loud.' WHERE id = ?1",
                [original.id.to_string()],
            )
            .unwrap();
            let copy_tone: String = tx
                .query_row(
                    "SELECT tone FROM persona WHERE id = ?1",
                    [copy.to_string()],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(copy_tone, "Loud.", "written in the same transaction");
            assert!(
                tx.execute("INSERT INTO persona (id) VALUES ('broken')", [])
                    .is_err()
            );
            // Dropped without a commit: both roll back.
        }
        assert_eq!(tone(&s.db, original.id), "Calm.");
        assert_eq!(tone(&s.db, copy), "Calm.");
    }

    #[test]
    fn losing_the_original_makes_copies_independent_with_their_content() {
        for loss in ["unmark", "delete", "delete project", "unlink"] {
            let s = setup();
            let (original, copy) = linked_narrator(&s);
            match loss {
                "unmark" => {
                    s.db.set_shareable(LibraryItem::Narrator(original.id), false)
                        .unwrap()
                }
                "delete" => {
                    s.db.conn()
                        .execute(
                            "DELETE FROM persona WHERE id = ?1",
                            [original.id.to_string()],
                        )
                        .unwrap();
                }
                "delete project" => {
                    s.db.conn()
                        .execute("DELETE FROM project WHERE id = ?1", [s.space.to_string()])
                        .unwrap();
                }
                _ => s.db.unlink(LibraryItem::Narrator(copy)).unwrap(),
            }
            let sharing = s.db.sharing(LibraryItem::Narrator(copy)).unwrap();
            assert!(!sharing.is_linked(), "{loss}");
            assert_eq!(tone(&s.db, copy), "Calm.", "{loss}");
            assert!(
                PersonaRepository::look(&s.db, copy).unwrap().is_some(),
                "{loss}: the look stays"
            );
            if loss == "unlink" {
                assert!(
                    sharing.lost().is_none(),
                    "the user's own unlink says nothing"
                );
            } else {
                assert_eq!(sharing.lost().unwrap().name, "Ana", "{loss} says so");
                s.db.forget_following(LibraryItem::Narrator(copy)).unwrap();
                assert_eq!(
                    s.db.sharing(LibraryItem::Narrator(copy)).unwrap(),
                    Sharing::default(),
                    "{loss}: once"
                );
            }
        }
    }

    #[test]
    fn only_a_projects_own_item_is_shareable_and_a_linked_copy_is_followed_by_none() {
        let s = setup();
        let (original, copy) = linked_narrator(&s);
        assert!(
            s.db.set_shareable(LibraryItem::Narrator(copy), true)
                .is_err()
        );
        assert!(
            s.db.copy_item(
                LibraryItem::Narrator(copy),
                Some(s.space),
                "Ana 2",
                CopyKind::Linked,
                at(6)
            )
            .is_err()
        );
        let items =
            s.db.shareable_items(s.owner, LibraryKind::Narrator)
                .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].item, LibraryItem::Narrator(original.id));
        assert_eq!(items[0].project, s.space);
        assert_eq!(
            s.db.linked_copies(LibraryItem::Narrator(original.id))
                .unwrap(),
            [LibraryItem::Narrator(copy)]
        );
        let listed =
            s.db.sharing_of(s.owner, Some(s.sea), LibraryKind::Narrator)
                .unwrap();
        assert_eq!(listed.len(), 1);
        assert!(listed[0].1.is_linked());
    }

    #[test]
    fn making_an_item_global_turns_its_linked_copies_into_references() {
        let s = setup();
        let (original, copy) = linked_narrator(&s);
        let channel = Channel::new(
            s.owner,
            s.sea,
            ChannelDetails::validate(ChannelDraft {
                name: "Deep Sea".into(),
                default_persona: Some(copy),
                ..ChannelDraft::default()
            })
            .unwrap(),
        );
        ChannelRepository::save(&s.db, &channel).unwrap();

        s.db.make_global(LibraryItem::Narrator(original.id), "Ana", at(7))
            .unwrap();
        let global = PersonaRepository::get(&s.db, original.id).unwrap().unwrap();
        assert_eq!(global.owner_project, None);
        assert!(
            PersonaRepository::get(&s.db, copy).unwrap().is_none(),
            "the copy's rows are dropped"
        );
        let channel = ChannelRepository::get(&s.db, channel.id).unwrap().unwrap();
        assert_eq!(channel.details.default_persona(), Some(original.id));
        assert!(
            s.db.linked_copies(LibraryItem::Narrator(original.id))
                .unwrap()
                .is_empty()
        );
    }

    fn product(s: &Setup) -> Product {
        let brief = ProductBrief::validate(ProductDraft {
            name: "Lamp".into(),
            kind: ProductKind::Physical,
            description: "A folding lamp.".into(),
            audience: "Campers.".into(),
            facts: "Folds flat".into(),
            offer: String::new(),
            link: "https://example.com/lamp".into(),
            call_to_action: "Get yours".into(),
            ownership: ProductOwnership::Mine,
            must_say: String::new(),
            never_say: String::new(),
        })
        .unwrap();
        let mut product = Product::new(s.owner, brief);
        product.owner_project = Some(s.space);
        ProductRepository::save(&s.db, &product).unwrap();
        product
    }

    fn product_image(product: ProductId, mark: u8) -> ProductImage {
        let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        bytes.extend(800_u32.to_be_bytes());
        bytes.extend(1000_u32.to_be_bytes());
        bytes.extend([8, 6, 0, 0, 0, mark]);
        ProductImage::new(
            product,
            ProductPicture::image(bytes).unwrap(),
            ProductImageKind::Photo,
            at(20),
        )
    }

    #[test]
    fn a_products_images_follow_and_keep_the_copys_ids() {
        let s = setup();
        let original = product(&s);
        let first = product_image(original.id, 1);
        ProductRepository::add_image(&s.db, &first).unwrap();
        s.db.set_shareable(LibraryItem::Product(original.id), true)
            .unwrap();
        let LibraryItem::Product(copy) =
            s.db.copy_item(
                LibraryItem::Product(original.id),
                Some(s.sea),
                "Lamp",
                CopyKind::Linked,
                at(5),
            )
            .unwrap()
        else {
            panic!("a product");
        };
        let copied = ProductRepository::images(&s.db, copy).unwrap();
        assert_eq!(copied.len(), 1);
        assert_ne!(copied[0].id, first.id, "ids of its own");

        ProductRepository::set_image_kind(&s.db, original.id, first.id, ProductImageKind::Screen)
            .unwrap();
        let again = ProductRepository::images(&s.db, copy).unwrap();
        assert_eq!(again[0].id, copied[0].id, "the copy's id stays");
        assert_eq!(again[0].kind, ProductImageKind::Screen);

        let second = product_image(original.id, 2);
        ProductRepository::add_image(&s.db, &second).unwrap();
        assert_eq!(ProductRepository::images(&s.db, copy).unwrap().len(), 2);
        ProductRepository::remove_image(&s.db, original.id, first.id).unwrap();
        assert_eq!(ProductRepository::images(&s.db, copy).unwrap().len(), 1);
    }

    #[test]
    fn a_world_items_kept_pictures_follow_and_a_waiting_one_stays_behind() {
        let s = setup();
        let mut original = WorldItem::new(
            s.owner,
            Some(s.space),
            WorldKind::Character,
            WorldItemDetails::validate(WorldItemDraft {
                name: "Rui".into(),
                appearance: "A fisherman.".into(),
                ..WorldItemDraft::default()
            })
            .unwrap(),
            at(3),
        );
        s.db.save_character(&original).unwrap();
        s.db.set_shareable(LibraryItem::World(original.id), true)
            .unwrap();
        let LibraryItem::World(copy) =
            s.db.copy_item(
                LibraryItem::World(original.id),
                Some(s.sea),
                "Rui",
                CopyKind::Linked,
                at(5),
            )
            .unwrap()
        else {
            panic!("a world item");
        };
        let picture = |kept: bool| ReferencePicture {
            id: ReferencePictureId::new(),
            item: original.id,
            look: bardo_domain::Look::Style(bardo_domain::VisualStyle::FilmNoir),
            file: format!("{}.png", Uuid::new_v4()),
            provenance: PictureProvenance {
                provider: Some(Provider::Gemini),
                model: "gemini-image".into(),
                prompt: "A portrait".into(),
                generated_at: at(4),
                job: None,
                origin: PictureOrigin::World,
                confirmed_at: None,
            },
            kept,
        };
        let waiting = picture(false);
        s.db.save_picture(&waiting).unwrap();
        assert!(s.db.pictures(copy).unwrap().is_empty());
        let kept = ReferencePicture {
            kept: true,
            ..waiting.clone()
        };
        s.db.save_picture(&kept).unwrap();
        let copied = s.db.pictures(copy).unwrap();
        assert_eq!(copied.len(), 1);
        assert_eq!(copied[0].file, kept.file);

        original.details = WorldItemDetails::validate(WorldItemDraft {
            name: "Rui".into(),
            appearance: "An old fisherman.".into(),
            ..WorldItemDraft::default()
        })
        .unwrap();
        s.db.save_character(&original).unwrap();
        let followed = s.db.character(copy).unwrap().unwrap();
        assert_eq!(followed.details.appearance(), "An old fisherman.");
        assert_eq!(followed.owner_project, Some(s.sea));
    }

    #[test]
    fn a_linked_template_takes_each_new_version_until_made_global() {
        let s = setup();
        let kind = TemplateKind::Script;
        let body =
            |prompt: &str| TemplateBody::new(kind, "Write in {{language}}.", prompt).unwrap();
        let mut first = TemplateVersion::first(s.owner, body("{{theme_title}} one"), at(1));
        first.owner_project = Some(s.space);
        s.db.add_template_version(&first).unwrap();
        let item = LibraryItem::template(s.space, kind);
        s.db.set_shareable(item, true).unwrap();
        assert_eq!(
            s.db.shareable_items(s.owner, LibraryKind::Template)
                .unwrap()
                .len(),
            1
        );
        let copy =
            s.db.copy_item(item, Some(s.sea), "", CopyKind::Linked, at(2))
                .unwrap();
        assert_eq!(copy, LibraryItem::template(s.sea, kind));
        let copied = s.db.template_versions(s.owner, Some(s.sea), kind).unwrap();
        assert_eq!(copied.len(), 1, "only the latest, as its first version");
        assert_eq!(copied[0].number, 1);

        let second = first.revise(body("{{theme_title}} two"), at(3)).unwrap();
        s.db.add_template_version(&second).unwrap();
        let copied = s.db.template_versions(s.owner, Some(s.sea), kind).unwrap();
        assert_eq!(copied.len(), 2);
        assert_eq!(copied[1].body, second.body);
        assert!(s.db.sharing(copy).unwrap().is_linked());

        s.db.make_global(item, "", at(4)).unwrap();
        for project in [s.space, s.sea] {
            assert!(
                s.db.template_versions(s.owner, Some(project), kind)
                    .unwrap()
                    .is_empty(),
                "both use the global template again"
            );
            assert_eq!(
                s.db.last_template_number(s.owner, Some(project), kind)
                    .unwrap(),
                2,
                "numbers are never taken twice"
            );
        }
        let global = s.db.template_versions(s.owner, None, kind).unwrap();
        assert_eq!(global.last().unwrap().body, second.body);
        assert_eq!(s.db.sharing(copy).unwrap(), Sharing::default());
        // Generations keep pointing at the version they used.
        assert!(s.db.template_version(second.id).unwrap().is_some());
    }

    #[test]
    fn every_column_of_an_item_is_followed_or_kept_on_purpose() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        let sql = mirror_triggers(&conn).unwrap();
        for item in [&PERSONA, &PRODUCT, &CHARACTER] {
            for column in columns(&conn, item.table).unwrap() {
                let kept = OWN.contains(&column.as_str()) || item.own.contains(&column.as_str());
                let followed = sql.contains(&format!("{column} = NEW.{column}"));
                assert!(
                    kept || followed,
                    "{}.{column} is neither followed nor kept",
                    item.table
                );
            }
        }
    }
}
