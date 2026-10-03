-- AI cut suggestions (#30): points in a project's narration where the
-- picture could change, as the decision engine scored them, and the score
-- a suggestion needs to show.

-- The score (0-100) below which suggestions are hidden unless asked for.
ALTER TABLE user_profile ADD COLUMN cut_suggestion_floor INTEGER NOT NULL DEFAULT 50
    CHECK (cut_suggestion_floor BETWEEN 0 AND 100);

-- A project's latest request. A new one replaces it with its suggestions
-- (ON DELETE CASCADE). No foreign key to the narration: a new narration
-- leaves the suggestions behind, and the editor stops showing them.
CREATE TABLE cut_suggestion_set (
    project_id   TEXT PRIMARY KEY NOT NULL
                 REFERENCES video_project (id) ON DELETE CASCADE,
    profile_id   TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    narration_id TEXT NOT NULL,
    job_id       TEXT NOT NULL,
    model        TEXT NOT NULL,
    -- Unix time in milliseconds.
    made_at      INTEGER NOT NULL
) STRICT;

-- Each suggestion, in narration order. The point is in the narration file,
-- in nanoseconds. Accepted is not stored: a suggestion is accepted while
-- the cut has a cut at its point.
CREATE TABLE cut_suggestion (
    project_id    TEXT NOT NULL
                  REFERENCES cut_suggestion_set (project_id) ON DELETE CASCADE,
    position      INTEGER NOT NULL CHECK (position >= 0),
    source_ns     INTEGER NOT NULL CHECK (source_ns >= 0),
    sentence_end  INTEGER NOT NULL CHECK (sentence_end IN (0, 1)),
    -- NULL when the narrator does not pause there.
    pause_ns      INTEGER CHECK (pause_ns >= 0),
    scene_change  INTEGER NOT NULL CHECK (scene_change IN (0, 1)),
    topic_shift   INTEGER NOT NULL CHECK (topic_shift IN (0, 1)),
    score         INTEGER NOT NULL CHECK (score BETWEEN 0 AND 100),
    confidence    REAL NOT NULL CHECK (confidence BETWEEN 0 AND 1),
    -- 'pending' or 'rejected'; others read as pending.
    status        TEXT NOT NULL,
    PRIMARY KEY (project_id, position)
) STRICT, WITHOUT ROWID;
