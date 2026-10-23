-- Performed clips on the cut (#193): where each performed clip belongs in
-- the narration, where the performer's face is in its picture, and whether
-- a project's new rough cut puts its performed clips on.

-- The narration a performed clip was made on and its take's stretch of it,
-- in milliseconds of the narration file; NULL for a clip whose capture was
-- gone before this column existed.
ALTER TABLE performed_clip ADD COLUMN narration_id TEXT;
ALTER TABLE performed_clip ADD COLUMN take_start_ms INTEGER CHECK (take_start_ms >= 0);
ALTER TABLE performed_clip ADD COLUMN take_end_ms INTEGER
    CHECK (take_end_ms > take_start_ms
           AND (narration_id IS NULL) = (take_start_ms IS NULL)
           AND (take_start_ms IS NULL) = (take_end_ms IS NULL));

-- The face's center over the take, in thousandths of the picture across
-- and down, when its take model keeps the take's framing.
ALTER TABLE performed_clip ADD COLUMN face_x INTEGER CHECK (face_x BETWEEN 0 AND 1000);
ALTER TABLE performed_clip ADD COLUMN face_y INTEGER
    CHECK (face_y BETWEEN 0 AND 1000 AND (face_x IS NULL) = (face_y IS NULL));

-- Clips used before: their capture's narration and their take as it is
-- now, while the capture is still there and the take has not changed
-- since its final was made (its control video is the one the final was
-- made from).
UPDATE performed_clip
SET narration_id = (
        SELECT capture.narration_id FROM capture
        WHERE capture.id = performed_clip.capture_id),
    take_start_ms = (
        SELECT capture_take.start_ms FROM capture_take
        JOIN capture ON capture.project_id = capture_take.project_id
        WHERE capture.id = performed_clip.capture_id
          AND capture_take.position = performed_clip.position),
    take_end_ms = (
        SELECT capture_take.end_ms FROM capture_take
        JOIN capture ON capture.project_id = capture_take.project_id
        WHERE capture.id = performed_clip.capture_id
          AND capture_take.position = performed_clip.position)
WHERE EXISTS (
    SELECT 1 FROM capture_take
    JOIN capture ON capture.project_id = capture_take.project_id
    WHERE capture.id = performed_clip.capture_id
      AND capture_take.position = performed_clip.position
      AND capture_take.end_ms > capture_take.start_ms)
  AND EXISTS (
    SELECT 1 FROM take_result
    JOIN control_video ON control_video.project_id = take_result.project_id
                      AND control_video.position = take_result.position
    WHERE take_result.project_id = performed_clip.project_id
      AND take_result.position = performed_clip.position
      AND take_result.step = 'final'
      AND control_video.capture_id = performed_clip.capture_id
      AND control_video.fingerprint = take_result.control);

-- A project whose new rough cut leaves its performed clips off; a project
-- without a row puts them on.
CREATE TABLE performed_rough_cut (
    project_id TEXT PRIMARY KEY NOT NULL REFERENCES video_project (id) ON DELETE CASCADE,
    on_cut     INTEGER NOT NULL CHECK (on_cut IN (0, 1))
) STRICT, WITHOUT ROWID;

-- Projects with performed clips already used keep the cut they play now:
-- one never edited would otherwise start over with the clips on it.
INSERT INTO performed_rough_cut (project_id, on_cut)
SELECT DISTINCT performed_clip.project_id, 0 FROM performed_clip
WHERE performed_clip.project_id NOT IN (SELECT project_id FROM timeline);
