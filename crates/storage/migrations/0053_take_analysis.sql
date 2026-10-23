-- Take analysis (#190): each take's mode (what its take model receives),
-- and the face mesh, depth map and person mask analyzed on the computer.

ALTER TABLE capture_take ADD COLUMN mode TEXT NOT NULL DEFAULT 'recording'
    CHECK (mode IN ('recording', 'mesh', 'depth'));

-- A take's finished analysis of one part: 'face' (the face mesh) or
-- 'depth' (the depth map and person mask). It names the capture and the
-- take's revision it was made from; another capture or revision makes it
-- stale. Its files are in the project folder, named after the capture,
-- the take and the revision.
CREATE TABLE take_analysis (
    project_id    TEXT NOT NULL REFERENCES capture (project_id) ON DELETE CASCADE,
    capture_id    TEXT NOT NULL,
    position      INTEGER NOT NULL CHECK (position >= 0),
    part          TEXT NOT NULL CHECK (part IN ('face', 'depth')),
    revision      INTEGER NOT NULL CHECK (revision >= 1),
    -- 'gpu' or 'cpu', and why the processor ran it when the graphics card
    -- was asked for.
    device        TEXT NOT NULL CHECK (device IN ('gpu', 'cpu')),
    fallback      TEXT CHECK (fallback IS NULL OR device = 'cpu'),
    -- The models that ran, as names and versions.
    model         TEXT NOT NULL,
    -- The checks, in frames.
    frames        INTEGER NOT NULL CHECK (frames >= 0),
    no_face       INTEGER NOT NULL CHECK (no_face >= 0),
    several_faces INTEGER NOT NULL CHECK (several_faces >= 0),
    small_face    INTEGER NOT NULL CHECK (small_face >= 0),
    turned        INTEGER NOT NULL CHECK (turned >= 0),
    mask_jumps    INTEGER NOT NULL CHECK (mask_jumps >= 0),
    -- Depth only: the take's far and near depth, in 16-bit steps.
    depth_far     INTEGER CHECK (depth_far BETWEEN 0 AND 65535),
    depth_near    INTEGER CHECK (depth_near BETWEEN 0 AND 65535),
    -- Unix time in milliseconds.
    finished_at   INTEGER NOT NULL,
    job_id        TEXT,
    CHECK ((part = 'depth') = (depth_far IS NOT NULL AND depth_near IS NOT NULL)),
    PRIMARY KEY (project_id, position, part)
) STRICT, WITHOUT ROWID;
