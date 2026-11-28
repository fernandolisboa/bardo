-- A video's own render settings (issue #360): what the user changed in
-- the Render stage over the account's or the network's preset, for this
-- video only. NULL keeps that value. Keyed by network rather than by
-- account, so a network with no account behind it (a file target) has
-- them too, and they outlive an account added or removed later.
CREATE TABLE render_setting (
    project_id        TEXT NOT NULL REFERENCES video_project (id) ON DELETE CASCADE,
    network           TEXT NOT NULL,
    aspect            TEXT,
    resolution        INTEGER CHECK (resolution > 0),
    codec             TEXT,
    bitrate_kbps      INTEGER CHECK (bitrate_kbps > 0),
    max_duration_secs INTEGER CHECK (max_duration_secs > 0),
    loudness_tenths   INTEGER,
    PRIMARY KEY (project_id, network)
) STRICT;
