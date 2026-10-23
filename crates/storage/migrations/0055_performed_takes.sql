-- Performed takes (#192): the take model each take goes to, the prompt it
-- is made from, the drafts and finals a take model made for it, and the
-- performed clips the user chose.

-- The take model a channel's takes use unless a take picks another; both
-- NULL for the app's default.
ALTER TABLE channel ADD COLUMN take_provider TEXT;
ALTER TABLE channel ADD COLUMN take_model TEXT
    CHECK ((take_provider IS NULL) = (take_model IS NULL));

-- A take's own take model; both NULL follow the channel's.
ALTER TABLE capture_take ADD COLUMN take_provider TEXT;
ALTER TABLE capture_take ADD COLUMN take_model TEXT
    CHECK ((take_provider IS NULL) = (take_model IS NULL));

-- The prompt of the take at `position`, written for one control video
-- (its fingerprint) and take model; the app compares both with the take as
-- it is now.
CREATE TABLE take_prompt (
    project_id      TEXT NOT NULL REFERENCES capture (project_id) ON DELETE CASCADE,
    capture_id      TEXT NOT NULL,
    position        INTEGER NOT NULL CHECK (position >= 0),
    control         TEXT NOT NULL,
    take_provider   TEXT NOT NULL,
    take_model      TEXT NOT NULL,
    text            TEXT NOT NULL CHECK (length(text) > 0),
    template_id     TEXT,
    template_number INTEGER CHECK (template_number >= 1),
    -- The model that wrote it.
    writer          TEXT NOT NULL,
    -- Unix time in milliseconds.
    written_at      INTEGER NOT NULL,
    job_id          TEXT,
    edited          INTEGER NOT NULL CHECK (edited IN (0, 1)),
    CHECK ((template_id IS NULL) = (template_number IS NULL)),
    PRIMARY KEY (project_id, position)
) STRICT, WITHOUT ROWID;

-- The latest draft and final a take model made for the take at
-- `position`, with how each was made. The file is in the project folder.
CREATE TABLE take_result (
    project_id      TEXT NOT NULL REFERENCES capture (project_id) ON DELETE CASCADE,
    capture_id      TEXT NOT NULL,
    position        INTEGER NOT NULL CHECK (position >= 0),
    step            TEXT NOT NULL CHECK (step IN ('draft', 'final')),
    control         TEXT NOT NULL,
    take_provider   TEXT NOT NULL,
    take_model      TEXT NOT NULL,
    -- The model and tier as Costs records it.
    priced_model    TEXT NOT NULL,
    prompt          TEXT NOT NULL,
    template_id     TEXT,
    template_number INTEGER CHECK (template_number >= 1),
    look            TEXT NOT NULL,
    realistic       INTEGER NOT NULL CHECK (realistic IN (0, 1)),
    request         TEXT NOT NULL,
    cost_micros     INTEGER CHECK (cost_micros >= 0),
    file            TEXT NOT NULL,
    duration_ms     INTEGER NOT NULL CHECK (duration_ms > 0),
    -- Unix time in milliseconds.
    made_at         INTEGER NOT NULL,
    job_id          TEXT,
    -- A draft approved, a final last used.
    accepted_at     INTEGER,
    CHECK ((template_id IS NULL) = (template_number IS NULL)),
    PRIMARY KEY (project_id, position, step)
) STRICT, WITHOUT ROWID;

-- A media asset can now be a take's final the user chose. SQLite cannot
-- change a CHECK in place, so the table is rebuilt (as in 0042).
CREATE TABLE media_asset_new (
    id          TEXT PRIMARY KEY NOT NULL,
    project_id  TEXT NOT NULL REFERENCES video_project (id) ON DELETE CASCADE,
    profile_id  TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    kind        TEXT NOT NULL CHECK (kind IN ('audio', 'video')),
    source      TEXT NOT NULL CHECK (source IN ('imported', 'generated', 'performed')),
    file        TEXT NOT NULL,
    name        TEXT NOT NULL,
    duration_ns INTEGER NOT NULL CHECK (duration_ns > 0),
    width       INTEGER CHECK (width > 0),
    height      INTEGER CHECK (height > 0),
    -- Unix time in milliseconds.
    imported_at INTEGER NOT NULL,
    CHECK ((kind = 'video') = (width IS NOT NULL)),
    CHECK ((width IS NULL) = (height IS NULL)),
    UNIQUE (project_id, file)
) STRICT;

INSERT INTO media_asset_new
    (id, project_id, profile_id, kind, source, file, name, duration_ns, width, height,
     imported_at)
SELECT id, project_id, profile_id, kind, source, file, name, duration_ns, width, height,
    imported_at
FROM media_asset;

DROP TABLE media_asset;

ALTER TABLE media_asset_new RENAME TO media_asset;

CREATE INDEX media_asset_project ON media_asset (project_id, imported_at);

-- A performed clip: the asset a used final became and the take it
-- performs. It outlives the capture, as the asset does.
CREATE TABLE performed_clip (
    asset_id      TEXT PRIMARY KEY NOT NULL REFERENCES media_asset (id) ON DELETE CASCADE,
    project_id    TEXT NOT NULL REFERENCES video_project (id) ON DELETE CASCADE,
    capture_id    TEXT NOT NULL,
    position      INTEGER NOT NULL CHECK (position >= 0),
    take_provider TEXT NOT NULL,
    take_model    TEXT NOT NULL,
    -- The look reads as a real person: the cut needs the disclosure.
    realistic     INTEGER NOT NULL CHECK (realistic IN (0, 1)),
    -- Unix time in milliseconds.
    used_at       INTEGER NOT NULL
) STRICT;

CREATE INDEX performed_clip_project ON performed_clip (project_id, used_at);

-- Templates of a new kind. Rebuilt as in 0051.
CREATE TABLE template_version_new (
    id           TEXT PRIMARY KEY NOT NULL,
    profile_id   TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    kind         TEXT NOT NULL
                 CHECK (kind IN ('script', 'image_prompt', 'music_prompt', 'metadata',
                                 'thumbnail', 'chapters', 'localization', 'look_prompt',
                                 'take_prompt')),
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
