CREATE TABLE user_profile (
    id          TEXT PRIMARY KEY NOT NULL,
    ui_language TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
