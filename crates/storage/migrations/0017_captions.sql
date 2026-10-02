-- Captions (#23): each saved cut's caption lines, whether they are burned
-- in and their style, and the style each channel's new projects start with.

-- 0 for a cut saved before captions existed: it gets them from the
-- narration's words when it is opened.
ALTER TABLE timeline ADD COLUMN has_captions INTEGER NOT NULL DEFAULT 0
    CHECK (has_captions IN (0, 1));
ALTER TABLE timeline ADD COLUMN captions_shown INTEGER NOT NULL DEFAULT 1
    CHECK (captions_shown IN (0, 1));
ALTER TABLE timeline ADD COLUMN caption_style TEXT NOT NULL DEFAULT 'clean'
    CHECK (caption_style IN ('clean', 'boxed', 'punch'));

-- One row per caption, in order. Times are in nanoseconds of the narration
-- file, so a caption shows wherever the cut plays its words.
CREATE TABLE timeline_caption (
    project_id TEXT NOT NULL REFERENCES timeline (project_id) ON DELETE CASCADE,
    position   INTEGER NOT NULL CHECK (position >= 0),
    text       TEXT NOT NULL CHECK (length(text) > 0),
    start_ns   INTEGER NOT NULL CHECK (start_ns >= 0),
    end_ns     INTEGER NOT NULL CHECK (end_ns > start_ns),
    PRIMARY KEY (project_id, position)
) STRICT, WITHOUT ROWID;

ALTER TABLE channel ADD COLUMN caption_style TEXT NOT NULL DEFAULT 'clean'
    CHECK (caption_style IN ('clean', 'boxed', 'punch'));
