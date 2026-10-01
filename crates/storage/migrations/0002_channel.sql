CREATE TABLE channel (
    id              TEXT PRIMARY KEY NOT NULL,
    profile_id      TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    niche           TEXT NOT NULL,
    aesthetic_notes TEXT NOT NULL,
    language        TEXT NOT NULL,
    country         TEXT NOT NULL,
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

-- Backstop for the app's own check; NOCASE folds ASCII only.
CREATE UNIQUE INDEX channel_profile_name ON channel (profile_id, name COLLATE NOCASE);

CREATE TABLE channel_theme (
    channel_id TEXT NOT NULL REFERENCES channel (id) ON DELETE CASCADE,
    position   INTEGER NOT NULL,
    theme      TEXT NOT NULL,
    PRIMARY KEY (channel_id, position)
) STRICT;
