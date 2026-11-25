-- Scenes that follow one another (#258): a scene plan's places, the
-- recurring places its scenes happen in, each with one fixed description
-- (as planned and as the user edited it) that every scene there is drawn
-- with.
CREATE TABLE scene_place (
    plan_id               TEXT NOT NULL REFERENCES scene_plan (id) ON DELETE CASCADE,
    position              INTEGER NOT NULL CHECK (position >= 0),
    name                  TEXT NOT NULL,
    generated_description TEXT NOT NULL,
    description           TEXT NOT NULL,
    PRIMARY KEY (plan_id, position)
) STRICT, WITHOUT ROWID;

-- Per scene: the place it happens in, as a position in the plan's places
-- (NULL for a scene somewhere of its own, and in plans made before).
ALTER TABLE scene ADD COLUMN place INTEGER CHECK (place IS NULL OR place >= 0);
