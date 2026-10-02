-- Export (#28): each network's metadata for a video project, the exports
-- written from it, the metadata template, and whether a persona's voice
-- needs a synthetic-content disclosure.

-- A clone of a real person or a realistic synthetic voice.
ALTER TABLE persona ADD COLUMN realistic_voice INTEGER NOT NULL DEFAULT 0
    CHECK (realistic_voice IN (0, 1));

-- Templates of a new kind: per-network metadata. Rebuilt as in 0019.
CREATE TABLE template_version_new (
    id           TEXT PRIMARY KEY NOT NULL,
    profile_id   TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    kind         TEXT NOT NULL
                 CHECK (kind IN ('script', 'image_prompt', 'music_prompt', 'metadata')),
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

-- One network's title, description and tags for a video project, as
-- Claude wrote them or as the user edited them. One generation writes
-- every network's; generating again replaces them all.
CREATE TABLE video_metadata (
    project_id    TEXT NOT NULL REFERENCES video_project (id) ON DELETE CASCADE,
    network       TEXT NOT NULL,
    profile_id    TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    title         TEXT NOT NULL,
    description   TEXT NOT NULL,
    -- One tag per line, without '#' (tags hold no line breaks).
    tags          TEXT NOT NULL,
    generation_id TEXT NOT NULL REFERENCES generation (id),
    edited        INTEGER NOT NULL CHECK (edited IN (0, 1)),
    -- Unix time in milliseconds.
    updated_at    INTEGER NOT NULL,
    PRIMARY KEY (project_id, network)
) STRICT, WITHOUT ROWID;

-- The latest export of each network of a video project: the package and
-- file written, and the render and post (fingerprint) it was made from. No
-- foreign key to the render: a new render replaces its row.
CREATE TABLE export (
    project_id  TEXT NOT NULL REFERENCES video_project (id) ON DELETE CASCADE,
    network     TEXT NOT NULL,
    profile_id  TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    account_id  TEXT NOT NULL,
    package     TEXT NOT NULL,
    video_file  TEXT NOT NULL,
    render_id   TEXT NOT NULL,
    post        TEXT NOT NULL,
    -- Unix time in milliseconds.
    exported_at INTEGER NOT NULL,
    PRIMARY KEY (project_id, network)
) STRICT, WITHOUT ROWID;
