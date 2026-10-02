-- A video project's cut as the user left it in the editor, on the scene
-- plan and narration it was made on. No foreign keys to those: a new plan
-- or narration replaces them, and the editor then starts over from the
-- rough cut.
CREATE TABLE timeline (
    project_id    TEXT PRIMARY KEY NOT NULL
                  REFERENCES video_project (id) ON DELETE CASCADE,
    profile_id    TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    scene_plan_id TEXT NOT NULL,
    narration_id  TEXT NOT NULL,
    -- Unix time in milliseconds.
    updated_at    INTEGER NOT NULL
) STRICT;

-- The items of each track, in order. Times are in nanoseconds: where in
-- its source the item starts, where on the timeline (video items run back
-- to back, so theirs follows from the lengths) and how long it plays. A
-- video item shows a scene of the plan, by position; an audio item plays a
-- file of the project folder.
CREATE TABLE timeline_item (
    project_id  TEXT NOT NULL REFERENCES timeline (project_id) ON DELETE CASCADE,
    track       TEXT NOT NULL CHECK (track IN ('video', 'narration')),
    position    INTEGER NOT NULL CHECK (position >= 0),
    scene       INTEGER CHECK (scene >= 0),
    file        TEXT,
    start_ns    INTEGER NOT NULL CHECK (start_ns >= 0),
    at_ns       INTEGER NOT NULL CHECK (at_ns >= 0),
    duration_ns INTEGER NOT NULL CHECK (duration_ns > 0),
    CHECK ((track = 'video') = (scene IS NOT NULL)),
    CHECK ((track = 'video') = (file IS NULL)),
    PRIMARY KEY (project_id, track, position)
) STRICT, WITHOUT ROWID;
