-- How an ad's product shots look (#285, ADR-0041).
--
-- The look the user chose for a scene's product shot, as one line:
-- layout, place and backdrop separated by '|' (read by the domain). NULL
-- is the look every shot gets first.
ALTER TABLE scene ADD COLUMN look TEXT;
-- The look the composed picture was made in. NULL for a picture made
-- before looks: it is composed again, for free.
ALTER TABLE scene ADD COLUMN shot_look TEXT;
-- Tones of the product image the picture was composed from, for the user
-- to build a backdrop from: hex colors separated by commas.
ALTER TABLE scene ADD COLUMN shot_tones TEXT;

-- Backdrops the image generator drew for a project's product shots: an
-- empty surface or set in the video's style. One waits for the user's
-- review until it is kept (kept = 1); every composed scene of the project
-- may then show it.
CREATE TABLE shot_backdrop (
    id         TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES video_project (id) ON DELETE CASCADE,
    file       TEXT NOT NULL,
    prompt     TEXT NOT NULL,
    provider   TEXT NOT NULL,
    model      TEXT NOT NULL,
    job_id     TEXT,
    -- Unix time in milliseconds.
    drawn_at   INTEGER NOT NULL,
    kept       INTEGER NOT NULL DEFAULT 0 CHECK (kept IN (0, 1))
) STRICT;

CREATE INDEX shot_backdrop_project ON shot_backdrop (project_id, drawn_at);
