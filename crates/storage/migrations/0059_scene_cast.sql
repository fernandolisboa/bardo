-- The same people from scene to scene (#246): a scene plan's cast, the
-- recurring characters its scenes show, each with a fixed appearance (as
-- planned and as the user edited it) and the reference portrait every
-- scene showing them is drawn with.
CREATE TABLE scene_cast (
    plan_id                TEXT NOT NULL REFERENCES scene_plan (id) ON DELETE CASCADE,
    position               INTEGER NOT NULL CHECK (position >= 0),
    name                   TEXT NOT NULL,
    generated_description  TEXT NOT NULL,
    description            TEXT NOT NULL,
    portrait_file          TEXT,
    portrait_generation_id TEXT REFERENCES generation (id),
    failure                TEXT,
    CHECK ((portrait_file IS NULL) = (portrait_generation_id IS NULL)),
    PRIMARY KEY (plan_id, position)
) STRICT, WITHOUT ROWID;

-- Per scene: the cast members it shows, as positions in the plan's cast
-- separated by commas, in the scene's order; and the motion prompt Claude
-- planned (NULL in plans made before, which fall back to the image
-- prompt).
ALTER TABLE scene ADD COLUMN cast_members TEXT NOT NULL DEFAULT '';
ALTER TABLE scene ADD COLUMN planned_motion_prompt TEXT;
