-- AI bridges over a cut in the editor (issue #363, ADR-0050): a clip a
-- video model makes from the frame before a cut to the frame after it,
-- covering the end of one picture and the start of the next. It waits
-- here, on its cut, until the user uses or discards it. The cut is kept
-- as the two pictures around it (`before_key`, `after_key`), so a cut
-- trimmed, moved or removed since no longer matches. Each frame is read
-- from a video file at a time (`*_at_ns`) or a still image (NULL time).
CREATE TABLE bridge (
    id            TEXT PRIMARY KEY,
    project_id    TEXT NOT NULL REFERENCES video_project (id) ON DELETE CASCADE,
    profile_id    TEXT NOT NULL REFERENCES user_profile (id),
    before_key    TEXT NOT NULL,
    after_key     TEXT NOT NULL,
    from_frame    INTEGER NOT NULL CHECK (from_frame >= 0),
    to_frame      INTEGER NOT NULL CHECK (to_frame > from_frame),
    first_file    TEXT NOT NULL,
    first_at_ns   INTEGER CHECK (first_at_ns >= 0),
    last_file     TEXT NOT NULL,
    last_at_ns    INTEGER CHECK (last_at_ns >= 0),
    provider      TEXT NOT NULL,
    model         TEXT NOT NULL,
    seconds       INTEGER NOT NULL CHECK (seconds > 0),
    note          TEXT NOT NULL,
    job_id        TEXT NOT NULL,
    file          TEXT,
    asked_at      INTEGER NOT NULL
) STRICT;

CREATE INDEX bridge_project ON bridge (project_id);
