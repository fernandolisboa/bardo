-- Retention over the cut (#183, ADR-0022): what each rendered file was cut
-- from, kept while the project's render, a post or an export points to it,
-- so a later render of the same account does not lose what a post was made
-- of. Codes are checked by the domain.
CREATE TABLE rendered_cut (
    render_id  TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES video_project (id) ON DELETE CASCADE,
    -- How long the file runs, and the shape it was rendered in.
    length_ns  INTEGER NOT NULL CHECK (length_ns > 0),
    aspect     TEXT NOT NULL,
    -- The render plan's fingerprint, as `render.cut`.
    cut        TEXT NOT NULL,
    -- Where the cut's clips and narration sat; NULL for files rendered
    -- before it was recorded.
    timing     TEXT
) STRICT;

CREATE INDEX rendered_cut_project ON rendered_cut (project_id);

-- The files rendered so far, without their timing.
INSERT INTO rendered_cut (render_id, project_id, length_ns, aspect, cut, timing)
SELECT id, project_id, duration_ns, aspect, cut, NULL FROM render;
