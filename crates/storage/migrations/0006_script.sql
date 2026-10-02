-- Versioned, editable templates. A saved version never changes; editing
-- adds the next number.
CREATE TABLE template_version (
    id           TEXT PRIMARY KEY NOT NULL,
    profile_id   TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    kind         TEXT NOT NULL CHECK (kind IN ('script')),
    number       INTEGER NOT NULL CHECK (number >= 1),
    instructions TEXT NOT NULL,
    prompt       TEXT NOT NULL,
    -- Unix time in milliseconds.
    created_at   INTEGER NOT NULL,
    UNIQUE (profile_id, kind, number)
) STRICT;

-- Provenance of every generated asset: provider, model, the final prompt,
-- the template version and the tokens used. Never changes once saved.
CREATE TABLE generation (
    id                  TEXT PRIMARY KEY NOT NULL,
    profile_id          TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    project_id          TEXT NOT NULL REFERENCES video_project (id) ON DELETE CASCADE,
    provider            TEXT NOT NULL,
    model               TEXT NOT NULL,
    template_version_id TEXT NOT NULL REFERENCES template_version (id),
    instructions        TEXT NOT NULL,
    prompt              TEXT NOT NULL,
    output              TEXT NOT NULL,
    input_tokens        INTEGER NOT NULL CHECK (input_tokens >= 0),
    output_tokens       INTEGER NOT NULL CHECK (output_tokens >= 0),
    -- Unix time in milliseconds.
    generated_at        INTEGER NOT NULL,
    -- The job that ran it; no foreign key, jobs may be cleaned up.
    job_id              TEXT
) STRICT;

CREATE INDEX generation_project ON generation (project_id);

-- A video project's script: the current text (possibly edited), the
-- generation it came from, and a regenerated one waiting for review.
CREATE TABLE script (
    project_id            TEXT PRIMARY KEY NOT NULL
                          REFERENCES video_project (id) ON DELETE CASCADE,
    profile_id            TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    text                  TEXT NOT NULL,
    source_generation_id  TEXT NOT NULL REFERENCES generation (id),
    pending_generation_id TEXT REFERENCES generation (id),
    -- Unix time in milliseconds.
    updated_at            INTEGER NOT NULL
) STRICT;
