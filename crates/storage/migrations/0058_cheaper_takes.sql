-- Cheaper performed takes (#243, ADR-0027): a test take per capture, the
-- preflight checks' measures and overrides, the look's face height, and a
-- cap on what a video's takes may cost before Bardo asks.

-- A take result can now be the capture's test. SQLite cannot change a
-- CHECK in place, so the table is rebuilt (as in 0055).
CREATE TABLE take_result_new (
    project_id      TEXT NOT NULL REFERENCES capture (project_id) ON DELETE CASCADE,
    capture_id      TEXT NOT NULL,
    position        INTEGER NOT NULL CHECK (position >= 0),
    step            TEXT NOT NULL CHECK (step IN ('draft', 'final', 'test')),
    control         TEXT NOT NULL,
    take_provider   TEXT NOT NULL,
    take_model      TEXT NOT NULL,
    -- The model and tier as Costs records it.
    priced_model    TEXT NOT NULL,
    prompt          TEXT NOT NULL,
    template_id     TEXT,
    template_number INTEGER CHECK (template_number >= 1),
    look            TEXT NOT NULL,
    realistic       INTEGER NOT NULL CHECK (realistic IN (0, 1)),
    request         TEXT NOT NULL,
    cost_micros     INTEGER CHECK (cost_micros >= 0),
    file            TEXT NOT NULL,
    duration_ms     INTEGER NOT NULL CHECK (duration_ms > 0),
    -- Unix time in milliseconds.
    made_at         INTEGER NOT NULL,
    job_id          TEXT,
    -- A draft or test approved, a final last used.
    accepted_at     INTEGER,
    seed            INTEGER,
    deadline        INTEGER,
    CHECK ((template_id IS NULL) = (template_number IS NULL)),
    PRIMARY KEY (project_id, position, step)
) STRICT, WITHOUT ROWID;

INSERT INTO take_result_new
    (project_id, capture_id, position, step, control, take_provider, take_model,
     priced_model, prompt, template_id, template_number, look, realistic, request,
     cost_micros, file, duration_ms, made_at, job_id, accepted_at, seed, deadline)
SELECT project_id, capture_id, position, step, control, take_provider, take_model,
    priced_model, prompt, template_id, template_number, look, realistic, request,
    cost_micros, file, duration_ms, made_at, job_id, accepted_at, seed, deadline
FROM take_result;

DROP TABLE take_result;

ALTER TABLE take_result_new RENAME TO take_result;

-- What the preflight checks read from a take's face mesh: the face heights
-- summed in thousandths of the frame, the frames that cut off the head or
-- the upper body, and the frames whose face moved too fast. NULL for takes
-- analyzed before they were counted.
ALTER TABLE take_analysis ADD COLUMN face_height_sum INTEGER CHECK (face_height_sum >= 0);
ALTER TABLE take_analysis ADD COLUMN cut_off INTEGER CHECK (cut_off >= 0);
ALTER TABLE take_analysis ADD COLUMN fast INTEGER CHECK (fast >= 0);

-- The user's choice to send a take whose checks failed, for the control
-- video `control` (its fingerprint).
CREATE TABLE take_preflight_override (
    project_id    TEXT NOT NULL REFERENCES capture (project_id) ON DELETE CASCADE,
    capture_id    TEXT NOT NULL,
    position      INTEGER NOT NULL CHECK (position >= 0),
    control       TEXT NOT NULL,
    -- Unix time in milliseconds.
    overridden_at INTEGER NOT NULL,
    PRIMARY KEY (project_id, position)
) STRICT, WITHOUT ROWID;

-- How tall the face in a persona's look is, measured on the image whose
-- fingerprint `image` is: thousandths of the image's height, NULL when no
-- face was found. Kept apart from persona_look so packages and settings
-- files, which read looks, never carry it.
CREATE TABLE persona_look_face (
    persona_id  TEXT PRIMARY KEY NOT NULL REFERENCES persona (id) ON DELETE CASCADE,
    image       TEXT NOT NULL,
    face_height INTEGER CHECK (face_height BETWEEN 0 AND 1000)
) STRICT, WITHOUT ROWID;

-- What a video's performed takes may cost before Bardo asks, in
-- micro-dollars; NULL for the default.
ALTER TABLE video_project ADD COLUMN take_cap_micros INTEGER CHECK (take_cap_micros > 0);
