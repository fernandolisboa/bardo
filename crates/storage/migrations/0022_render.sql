-- Final renders (#27): the latest file rendered for each network account
-- of a video project, in the preset it was rendered with. A new render for
-- the same account replaces the row (and the file, which keeps its name).
CREATE TABLE render (
    id                TEXT PRIMARY KEY NOT NULL,
    project_id        TEXT NOT NULL REFERENCES video_project (id) ON DELETE CASCADE,
    profile_id        TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    account_id        TEXT NOT NULL REFERENCES network_account (id) ON DELETE CASCADE,
    network           TEXT NOT NULL,
    -- The preset as rendered: every value, overrides merged.
    aspect            TEXT NOT NULL,
    resolution        INTEGER NOT NULL CHECK (resolution > 0),
    codec             TEXT NOT NULL,
    bitrate_kbps      INTEGER NOT NULL CHECK (bitrate_kbps > 0),
    max_duration_secs INTEGER NOT NULL CHECK (max_duration_secs > 0),
    loudness_tenths   INTEGER NOT NULL,
    -- The file in the project folder and how it came out.
    file              TEXT NOT NULL,
    encoder           TEXT NOT NULL,
    duration_ns       INTEGER NOT NULL CHECK (duration_ns > 0),
    size_bytes        INTEGER NOT NULL CHECK (size_bytes >= 0),
    -- Measured after rendering, in hundredths of LUFS and dBTP; NULL when
    -- the render is silent.
    integrated_cents  INTEGER,
    true_peak_cents   INTEGER,
    -- A fingerprint of the cut as rendered.
    cut               TEXT NOT NULL,
    -- Unix time in milliseconds.
    rendered_at       INTEGER NOT NULL,
    CHECK ((integrated_cents IS NULL) = (true_peak_cents IS NULL)),
    UNIQUE (project_id, account_id)
) STRICT;
