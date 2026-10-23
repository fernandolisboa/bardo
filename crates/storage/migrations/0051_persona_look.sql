-- Persona looks (#188): the image of the character a persona appears as,
-- kept in the database beside the persona so backups and restores always
-- carry them together. A persona has at most one look and one drawing
-- waiting for review beside it. Codes are checked by the domain.
CREATE TABLE persona_look (
    persona_id     TEXT NOT NULL REFERENCES persona (id) ON DELETE CASCADE,
    -- 'current' or 'pending' (a drawing waiting for review).
    slot           TEXT NOT NULL CHECK (slot IN ('current', 'pending')),
    image          BLOB NOT NULL,
    format         TEXT NOT NULL,
    width          INTEGER NOT NULL CHECK (width > 0),
    height         INTEGER NOT NULL CHECK (height > 0),
    source         TEXT NOT NULL,
    realistic      INTEGER NOT NULL CHECK (realistic IN (0, 1)),
    -- How a drawn look was made; NULL for an imported photo.
    description    TEXT,
    prompt         TEXT,
    template_id    TEXT,
    template_number INTEGER CHECK (template_number >= 1),
    writer_model   TEXT,
    image_model    TEXT,
    job_id         TEXT,
    -- Unix time in milliseconds.
    made_at        INTEGER NOT NULL,
    PRIMARY KEY (persona_id, slot),
    CHECK ((source = 'drawn') = (prompt IS NOT NULL))
) STRICT, WITHOUT ROWID;

-- Templates of a new kind. Rebuilt as in 0023.
CREATE TABLE template_version_new (
    id           TEXT PRIMARY KEY NOT NULL,
    profile_id   TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    kind         TEXT NOT NULL
                 CHECK (kind IN ('script', 'image_prompt', 'music_prompt', 'metadata',
                                 'thumbnail', 'chapters', 'localization', 'look_prompt')),
    number       INTEGER NOT NULL CHECK (number >= 1),
    instructions TEXT NOT NULL,
    prompt       TEXT NOT NULL,
    -- Unix time in milliseconds.
    created_at   INTEGER NOT NULL,
    UNIQUE (profile_id, kind, number)
) STRICT;

INSERT INTO template_version_new
    (id, profile_id, kind, number, instructions, prompt, created_at)
SELECT id, profile_id, kind, number, instructions, prompt, created_at
FROM template_version;

DROP TABLE template_version;

ALTER TABLE template_version_new RENAME TO template_version;
