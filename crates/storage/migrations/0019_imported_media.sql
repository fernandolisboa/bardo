-- Imported media (#25): audio and video files the user brings into a video
-- project, the music, SFX and video tracks they are placed on, and the
-- music prompt Claude writes for the user's music tool.

-- Each asset's copy lives in the project folder under `file`; `name` is
-- the file's name as the user had it. A video has the size of its picture.
CREATE TABLE media_asset (
    id          TEXT PRIMARY KEY NOT NULL,
    project_id  TEXT NOT NULL REFERENCES video_project (id) ON DELETE CASCADE,
    profile_id  TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    kind        TEXT NOT NULL CHECK (kind IN ('audio', 'video')),
    source      TEXT NOT NULL CHECK (source IN ('imported')),
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

CREATE INDEX media_asset_project ON media_asset (project_id, imported_at);

-- Timeline items again, so a video item can show imported footage (by its
-- file) instead of a scene, and the music and SFX tracks hold audio items.
-- SQLite cannot change a CHECK in place, so the table is rebuilt.
CREATE TABLE timeline_item_new (
    project_id  TEXT NOT NULL REFERENCES timeline (project_id) ON DELETE CASCADE,
    -- 'video', 'narration', 'music' or 'sfx'; later tracks add their own.
    track       TEXT NOT NULL,
    position    INTEGER NOT NULL CHECK (position >= 0),
    scene       INTEGER CHECK (scene >= 0),
    file        TEXT,
    start_ns    INTEGER NOT NULL CHECK (start_ns >= 0),
    at_ns       INTEGER NOT NULL CHECK (at_ns >= 0),
    duration_ns INTEGER NOT NULL CHECK (duration_ns > 0),
    fade_in_ns  INTEGER NOT NULL DEFAULT 0 CHECK (fade_in_ns >= 0),
    fade_out_ns INTEGER NOT NULL DEFAULT 0 CHECK (fade_out_ns >= 0),
    framing     TEXT NOT NULL DEFAULT 'crop' CHECK (framing IN ('crop', 'fit')),
    crop_x      INTEGER NOT NULL DEFAULT 500 CHECK (crop_x BETWEEN 0 AND 1000),
    crop_y      INTEGER NOT NULL DEFAULT 500 CHECK (crop_y BETWEEN 0 AND 1000),
    -- A video item shows a scene or a file; an audio item plays a file.
    CHECK ((scene IS NULL) <> (file IS NULL)),
    CHECK (track = 'video' OR scene IS NULL),
    PRIMARY KEY (project_id, track, position)
) STRICT, WITHOUT ROWID;

INSERT INTO timeline_item_new
    (project_id, track, position, scene, file, start_ns, at_ns, duration_ns, fade_in_ns,
     fade_out_ns, framing, crop_x, crop_y)
SELECT project_id, track, position, scene, file, start_ns, at_ns, duration_ns, fade_in_ns,
    fade_out_ns, framing, crop_x, crop_y
FROM timeline_item;

DROP TABLE timeline_item;

ALTER TABLE timeline_item_new RENAME TO timeline_item;

-- Templates of a new kind: the music prompt. Rebuilt as in 0009.
CREATE TABLE template_version_new (
    id           TEXT PRIMARY KEY NOT NULL,
    profile_id   TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    kind         TEXT NOT NULL CHECK (kind IN ('script', 'image_prompt', 'music_prompt')),
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

-- A video project's music prompt: the text to copy (possibly edited) and
-- the generation it came from. Generating again replaces it.
CREATE TABLE music_prompt (
    project_id    TEXT PRIMARY KEY NOT NULL
                  REFERENCES video_project (id) ON DELETE CASCADE,
    profile_id    TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    text          TEXT NOT NULL CHECK (length(text) > 0),
    generation_id TEXT NOT NULL REFERENCES generation (id),
    -- Unix time in milliseconds.
    updated_at    INTEGER NOT NULL
) STRICT;
