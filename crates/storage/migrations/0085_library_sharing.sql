-- Sharing library items between projects (issue #340, ADR-0047 item 5).
-- A project's narrator, product or world item is shared only when the
-- user marks it `shareable`. A linked copy names its original in
-- `linked_from` and is kept equal to it by triggers that Bardo writes on
-- every start (see `sharing.rs`), so every change reaches it in the same
-- transaction. `follows_name` and `follows_project` keep the original's
-- names: while `linked_from` is set they are shown as *Follows ...*; once
-- the original is lost (deleted, unmarked, its project deleted) they stay
-- until the user saw that the copy became independent.

ALTER TABLE persona ADD COLUMN shareable INTEGER NOT NULL DEFAULT 0
    CHECK (shareable IN (0, 1));
ALTER TABLE persona ADD COLUMN linked_from TEXT
    REFERENCES persona (id) ON DELETE SET NULL;
ALTER TABLE persona ADD COLUMN follows_name TEXT;
ALTER TABLE persona ADD COLUMN follows_project TEXT;
CREATE INDEX persona_linked_from ON persona (linked_from);

ALTER TABLE product ADD COLUMN shareable INTEGER NOT NULL DEFAULT 0
    CHECK (shareable IN (0, 1));
ALTER TABLE product ADD COLUMN linked_from TEXT
    REFERENCES product (id) ON DELETE SET NULL;
ALTER TABLE product ADD COLUMN follows_name TEXT;
ALTER TABLE product ADD COLUMN follows_project TEXT;
CREATE INDEX product_linked_from ON product (linked_from);

ALTER TABLE character ADD COLUMN shareable INTEGER NOT NULL DEFAULT 0
    CHECK (shareable IN (0, 1));
ALTER TABLE character ADD COLUMN linked_from TEXT
    REFERENCES character (id) ON DELETE SET NULL;
ALTER TABLE character ADD COLUMN follows_name TEXT;
ALTER TABLE character ADD COLUMN follows_project TEXT;
CREATE INDEX character_linked_from ON character (linked_from);

-- A linked copy's row of a table with ids of its own names the original's
-- row it mirrors, so the copy's ids stay put while it follows.
ALTER TABLE product_image ADD COLUMN copied_from TEXT;
ALTER TABLE product_claim ADD COLUMN copied_from TEXT;
ALTER TABLE product_concept ADD COLUMN copied_from TEXT;
ALTER TABLE reference_picture ADD COLUMN copied_from TEXT;

-- A project's template versions stop being its template when it is made
-- global (Unix time in milliseconds); generations keep pointing at them.
ALTER TABLE template_version ADD COLUMN retired_at INTEGER;

-- A project's template of a kind: shareable, or a linked copy of another
-- project's (its versions then take each new version of the original's).
CREATE TABLE template_share (
    owner_project_id TEXT NOT NULL REFERENCES project (id) ON DELETE CASCADE,
    kind             TEXT NOT NULL,
    shareable        INTEGER NOT NULL DEFAULT 0 CHECK (shareable IN (0, 1)),
    linked_from      TEXT REFERENCES project (id) ON DELETE SET NULL,
    follows_project  TEXT,
    PRIMARY KEY (owner_project_id, kind)
) STRICT, WITHOUT ROWID;

CREATE INDEX template_share_linked_from ON template_share (linked_from, kind);
