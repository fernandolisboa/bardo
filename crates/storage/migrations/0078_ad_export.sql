-- The export for ads (#283). An ad's texts for an ad manager: a primary
-- text and a headline, written by the ad metadata job and edited by the
-- user. Projects that are not ads have no row.
CREATE TABLE ad_texts (
    project_id   TEXT PRIMARY KEY REFERENCES video_project (id) ON DELETE CASCADE,
    primary_text TEXT NOT NULL,
    headline     TEXT NOT NULL,
    -- 1 once the user wrote them: writing the post texts again asks first.
    edited       INTEGER NOT NULL DEFAULT 0
) STRICT, WITHOUT ROWID;

-- Where the user moved a clip's window in one shape of the export; a
-- clip without a row is centered. A clip is its picture and where its
-- source starts.
CREATE TABLE ad_window (
    project_id TEXT NOT NULL REFERENCES video_project (id) ON DELETE CASCADE,
    -- '4:5', '1:1', '9:16' or '16:9'.
    shape      TEXT NOT NULL,
    clip       TEXT NOT NULL,
    -- Steps of 0 to 1000 across and down.
    x          INTEGER NOT NULL,
    y          INTEGER NOT NULL,
    PRIMARY KEY (project_id, shape, clip)
) STRICT, WITHOUT ROWID;

-- Each shape's last export: what it was made from, to tell when it is out
-- of date, and its video file.
CREATE TABLE ad_export (
    project_id  TEXT NOT NULL REFERENCES video_project (id) ON DELETE CASCADE,
    shape       TEXT NOT NULL,
    made_from   TEXT NOT NULL,
    file        TEXT NOT NULL,
    size_bytes  INTEGER NOT NULL,
    exported_at INTEGER NOT NULL,
    -- What its review went with (#282): the disclosure and the
    -- acknowledged items' keys, one per line.
    disclosure   TEXT NOT NULL,
    acknowledged TEXT NOT NULL,
    PRIMARY KEY (project_id, shape)
) STRICT, WITHOUT ROWID;
