CREATE TABLE job (
    id              TEXT PRIMARY KEY NOT NULL,
    profile_id      TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    kind            TEXT NOT NULL,
    payload         TEXT NOT NULL,
    state           TEXT NOT NULL,
    -- Thousandths, 0 to 1000.
    progress        INTEGER NOT NULL CHECK (progress BETWEEN 0 AND 1000),
    attempts        INTEGER NOT NULL CHECK (attempts >= 0),
    checkpoint      TEXT,
    external_handle TEXT,
    failure_kind    TEXT,
    failure_detail  TEXT,
    -- Unix time in milliseconds.
    retry_at        INTEGER,
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK ((failure_kind IS NULL) = (failure_detail IS NULL))
) STRICT;

CREATE INDEX job_profile ON job (profile_id);
