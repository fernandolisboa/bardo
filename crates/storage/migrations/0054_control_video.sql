-- Control videos (#191): each take's set (where the performer appears in
-- Depth mode) and the control video built for it, with the user's
-- approval.

-- A take's set image, copied into the project folder, and where it came
-- from; both empty for the recording's own background (the room).
ALTER TABLE capture_take ADD COLUMN set_file TEXT;
ALTER TABLE capture_take ADD COLUMN set_source TEXT
    CHECK ((set_source IS NULL) = (set_file IS NULL)
        AND (set_source IS NULL OR set_source IN ('scene', 'imported')));

-- The latest control video built for the take at `position`. Its
-- fingerprint names what it was made from (the capture, the take's bounds
-- and revision, mode, set, the narration and the analyses); the app
-- compares it with the take as it is now, so any change makes it stale and
-- its approval with it. The file is in the project folder.
CREATE TABLE control_video (
    project_id  TEXT NOT NULL REFERENCES capture (project_id) ON DELETE CASCADE,
    capture_id  TEXT NOT NULL,
    position    INTEGER NOT NULL CHECK (position >= 0),
    fingerprint TEXT NOT NULL,
    file        TEXT NOT NULL,
    bytes       INTEGER NOT NULL CHECK (bytes >= 0),
    frames      INTEGER NOT NULL CHECK (frames >= 0),
    -- Mesh and Depth: frames with no face near enough to cover.
    uncovered   INTEGER NOT NULL CHECK (uncovered >= 0),
    -- ffmpeg's name for the encoder that made it.
    encoder     TEXT NOT NULL,
    -- Unix time in milliseconds.
    built_at    INTEGER NOT NULL,
    job_id      TEXT,
    approved_at INTEGER,
    -- The build it replaced was approved, for the take as it was then:
    -- the review says so until this one is approved.
    replaced_approval INTEGER NOT NULL DEFAULT 0 CHECK (replaced_approval IN (0, 1)),
    PRIMARY KEY (project_id, position)
) STRICT, WITHOUT ROWID;
