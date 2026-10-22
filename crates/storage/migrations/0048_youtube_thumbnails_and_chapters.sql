-- YouTube thumbnails and chapters (#182, ADR-0021): each channel's look
-- for thumbnail text, each project's thumbnail and the backgrounds drawn
-- for it, each project's chapters, the two template kinds that write
-- them, and how sending a thumbnail with an upload went. Codes are checked
-- by the domain.

-- Thumbnails are new, so no existing channel changes by getting the
-- domain's default look.
ALTER TABLE channel ADD COLUMN thumbnail_look TEXT NOT NULL DEFAULT 'bold';

-- How sending the thumbnail went ('set', 'not_allowed', 'failed:<why>');
-- NULL until it was sent, and for uploads without one.
ALTER TABLE publication ADD COLUMN upload_thumbnail TEXT;

-- Templates of two new kinds. Rebuilt as in 0023.
CREATE TABLE template_version_new (
    id           TEXT PRIMARY KEY NOT NULL,
    profile_id   TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    kind         TEXT NOT NULL
                 CHECK (kind IN ('script', 'image_prompt', 'music_prompt', 'metadata',
                                 'thumbnail', 'chapters')),
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

-- A project's thumbnail: what the user picked (text, placement, look,
-- background), the prompt the next background is drawn from, the call
-- that last wrote the text and prompt (NULL when the user wrote them),
-- and the picture saving made (NULL until saved).
CREATE TABLE thumbnail (
    project_id        TEXT PRIMARY KEY NOT NULL
                      REFERENCES video_project (id) ON DELETE CASCADE,
    profile_id        TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    text              TEXT NOT NULL,
    placement         TEXT NOT NULL,
    look              TEXT NOT NULL,
    background_file   TEXT,
    background_source TEXT,
    prompt            TEXT NOT NULL,
    generation_id     TEXT REFERENCES generation (id),
    file              TEXT,
    -- Unix time in milliseconds.
    updated_at        INTEGER NOT NULL,
    CHECK ((background_file IS NULL) = (background_source IS NULL))
) STRICT;

-- A background the image provider drew for a project's thumbnail.
CREATE TABLE drawn_background (
    project_id TEXT NOT NULL REFERENCES video_project (id) ON DELETE CASCADE,
    file       TEXT NOT NULL,
    profile_id TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    prompt     TEXT NOT NULL,
    provider   TEXT NOT NULL,
    model      TEXT NOT NULL,
    job_id     TEXT,
    -- Unix time in milliseconds.
    drawn_at   INTEGER NOT NULL,
    PRIMARY KEY (project_id, file)
) STRICT, WITHOUT ROWID;

-- A project's chapters: whether they go in YouTube's description, the
-- narration their starts are times of (no foreign key: a new narration
-- replaces its row, and the chapters say they are out of date), the call
-- that wrote them and whether the user changed them.
CREATE TABLE chapters (
    project_id    TEXT PRIMARY KEY NOT NULL
                  REFERENCES video_project (id) ON DELETE CASCADE,
    profile_id    TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    include       INTEGER NOT NULL CHECK (include IN (0, 1)),
    narration_id  TEXT NOT NULL,
    generation_id TEXT NOT NULL REFERENCES generation (id),
    edited        INTEGER NOT NULL CHECK (edited IN (0, 1)),
    -- Unix time in milliseconds.
    updated_at    INTEGER NOT NULL
) STRICT;

-- One chapter, in order: where it starts in the narration file and its
-- title.
CREATE TABLE chapter (
    project_id TEXT NOT NULL REFERENCES chapters (project_id) ON DELETE CASCADE,
    position   INTEGER NOT NULL CHECK (position >= 0),
    start_ms   INTEGER NOT NULL CHECK (start_ms >= 0),
    title      TEXT NOT NULL CHECK (length(title) > 0),
    PRIMARY KEY (project_id, position)
) STRICT, WITHOUT ROWID;
