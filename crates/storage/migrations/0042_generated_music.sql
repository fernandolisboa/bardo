-- Generated music (#169): a media asset can now be a track Bardo made, and
-- each such track keeps where it came from and whether the user chose it.

-- SQLite cannot change a CHECK in place, so the table is rebuilt.
CREATE TABLE media_asset_new (
    id          TEXT PRIMARY KEY NOT NULL,
    project_id  TEXT NOT NULL REFERENCES video_project (id) ON DELETE CASCADE,
    profile_id  TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    kind        TEXT NOT NULL CHECK (kind IN ('audio', 'video')),
    source      TEXT NOT NULL CHECK (source IN ('imported', 'generated')),
    file        TEXT NOT NULL,
    name        TEXT NOT NULL,
    duration_ns INTEGER NOT NULL CHECK (duration_ns > 0),
    width       INTEGER CHECK (width > 0),
    height      INTEGER CHECK (height > 0),
    -- Unix time in milliseconds.
    imported_at INTEGER NOT NULL,
    CHECK ((kind = 'video') = (width IS NOT NULL)),
    CHECK ((width IS NULL) = (height IS NULL)),
    UNIQUE (project_id, file)
) STRICT;

INSERT INTO media_asset_new
    (id, project_id, profile_id, kind, source, file, name, duration_ns, width, height,
     imported_at)
SELECT id, project_id, profile_id, kind, source, file, name, duration_ns, width, height,
    imported_at
FROM media_asset;

DROP TABLE media_asset;

ALTER TABLE media_asset_new RENAME TO media_asset;

CREATE INDEX media_asset_project ON media_asset (project_id, imported_at);

-- A track Bardo made: the asset it is, the provider and model that made
-- it, the prompt and length asked for, and when the user chose it for the
-- cut (NULL while it waits for review).
CREATE TABLE music_track (
    asset_id     TEXT PRIMARY KEY NOT NULL REFERENCES media_asset (id) ON DELETE CASCADE,
    project_id   TEXT NOT NULL REFERENCES video_project (id) ON DELETE CASCADE,
    profile_id   TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    provider     TEXT NOT NULL,
    model        TEXT NOT NULL,
    prompt       TEXT NOT NULL CHECK (length(prompt) > 0),
    length_ms    INTEGER NOT NULL CHECK (length_ms > 0),
    job_id       TEXT,
    -- Unix time in milliseconds.
    generated_at INTEGER NOT NULL,
    chosen_at    INTEGER
) STRICT;

CREATE INDEX music_track_project ON music_track (project_id, generated_at);
