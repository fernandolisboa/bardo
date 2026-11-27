-- Narrators, products and templates owned by a project or global
-- (issue #339, ADR-0047). NULL is the global library, which every row from
-- before this migration joins, so nothing a channel or video used changes.
-- Deleting the project deletes its own items.

ALTER TABLE persona ADD COLUMN owner_project_id TEXT
    REFERENCES project (id) ON DELETE CASCADE;

DROP INDEX persona_profile_name;
CREATE UNIQUE INDEX persona_profile_name
    ON persona (profile_id, IFNULL(owner_project_id, ''), name COLLATE NOCASE);

ALTER TABLE product ADD COLUMN owner_project_id TEXT
    REFERENCES project (id) ON DELETE CASCADE;

DROP INDEX product_profile_name;
CREATE UNIQUE INDEX product_profile_name
    ON product (profile_id, IFNULL(owner_project_id, ''), name COLLATE NOCASE);

-- A template kind has a global template and may have a project's own:
-- version numbers count per owner. Rebuilt as in 0070, since the numbers'
-- uniqueness was part of the table.
CREATE TABLE template_version_new (
    id               TEXT PRIMARY KEY NOT NULL,
    profile_id       TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    owner_project_id TEXT REFERENCES project (id) ON DELETE CASCADE,
    kind             TEXT NOT NULL,
    number           INTEGER NOT NULL CHECK (number >= 1),
    instructions     TEXT NOT NULL,
    prompt           TEXT NOT NULL,
    -- Unix time in milliseconds.
    created_at       INTEGER NOT NULL
) STRICT;

INSERT INTO template_version_new
    (id, profile_id, owner_project_id, kind, number, instructions, prompt, created_at)
SELECT id, profile_id, NULL, kind, number, instructions, prompt, created_at
FROM template_version;

DROP TABLE template_version;

ALTER TABLE template_version_new RENAME TO template_version;

CREATE UNIQUE INDEX template_version_number
    ON template_version (profile_id, IFNULL(owner_project_id, ''), kind, number);
