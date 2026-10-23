-- Seedance 2.5 on BytePlus ModelArk (#194, ADR-0025): drafts finished from
-- their own task within a deadline, looks from the user's ModelArk asset
-- library, and the files Bardo stages in the user's own bucket.

-- The seed the provider reports for a draft or final, and until when a
-- draft can be finished (Unix time in milliseconds); NULL where the
-- provider says no seed or a final is a new take.
ALTER TABLE take_result ADD COLUMN seed INTEGER;
ALTER TABLE take_result ADD COLUMN deadline INTEGER;

-- A look's entry in a provider's asset library, used in place of its
-- image. It holds only for the image it was given for (`image`, a
-- fingerprint of it). Kept apart from persona_look so persona packages and
-- settings files, which read looks, never carry it.
CREATE TABLE persona_look_asset (
    persona_id TEXT NOT NULL REFERENCES persona (id) ON DELETE CASCADE,
    provider   TEXT NOT NULL,
    kind       TEXT NOT NULL,
    asset_id   TEXT NOT NULL CHECK (length(asset_id) BETWEEN 1 AND 128),
    image      TEXT NOT NULL,
    PRIMARY KEY (persona_id, provider)
) STRICT, WITHOUT ROWID;

-- Files staged in the user's own storage for take models, until they are
-- deleted. Not tied to the capture by a foreign key: a removed capture's
-- files must still be deleted.
CREATE TABLE take_staging (
    file        TEXT PRIMARY KEY NOT NULL,
    profile_id  TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    provider    TEXT NOT NULL,
    project_id  TEXT NOT NULL,
    capture_id  TEXT NOT NULL,
    position    INTEGER NOT NULL CHECK (position >= 0),
    job_id      TEXT NOT NULL,
    -- Unix time in milliseconds.
    staged_at   INTEGER NOT NULL,
    expires_at  INTEGER NOT NULL,
    released_at INTEGER
) STRICT;

CREATE INDEX take_staging_job ON take_staging (job_id);
CREATE INDEX take_staging_capture ON take_staging (capture_id);
