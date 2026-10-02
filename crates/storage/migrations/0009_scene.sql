-- Templates of a new kind: the image prompts of a scene plan. SQLite cannot
-- change a CHECK in place, so the table is rebuilt; generations keep
-- referring to it by name.
CREATE TABLE template_version_new (
    id           TEXT PRIMARY KEY NOT NULL,
    profile_id   TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    kind         TEXT NOT NULL CHECK (kind IN ('script', 'image_prompt')),
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

-- A video project's scene plan: the scenes Claude planned on the project's
-- narration. One per project; a new plan replaces the old one.
CREATE TABLE scene_plan (
    project_id    TEXT PRIMARY KEY NOT NULL
                  REFERENCES video_project (id) ON DELETE CASCADE,
    id            TEXT NOT NULL UNIQUE,
    profile_id    TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    -- The narration the scenes are timed on; no foreign key, a new
    -- narration replaces it and the plan goes stale.
    narration_id  TEXT NOT NULL,
    generation_id TEXT NOT NULL REFERENCES generation (id),
    -- Unix time in milliseconds.
    updated_at    INTEGER NOT NULL
) STRICT;

-- The scenes of a plan, in order: their time in the narration, the
-- narration spoken over them, the planned and the current image prompt,
-- the image (a file in the project folder and its generation), a newer
-- image waiting for review, and why the last attempt failed.
CREATE TABLE scene (
    plan_id               TEXT NOT NULL REFERENCES scene_plan (id) ON DELETE CASCADE,
    position              INTEGER NOT NULL CHECK (position >= 0),
    start_ms              INTEGER NOT NULL CHECK (start_ms >= 0),
    end_ms                INTEGER NOT NULL CHECK (end_ms >= start_ms),
    text                  TEXT NOT NULL,
    generated_prompt      TEXT NOT NULL,
    prompt                TEXT NOT NULL,
    image_file            TEXT,
    image_generation_id   TEXT REFERENCES generation (id),
    pending_file          TEXT,
    pending_generation_id TEXT REFERENCES generation (id),
    failure               TEXT,
    CHECK ((image_file IS NULL) = (image_generation_id IS NULL)),
    CHECK ((pending_file IS NULL) = (pending_generation_id IS NULL)),
    PRIMARY KEY (plan_id, position)
) STRICT, WITHOUT ROWID;
