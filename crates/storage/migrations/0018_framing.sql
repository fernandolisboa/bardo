-- Framing (#24): the frame shape each saved cut is made for, and how each
-- of its video items fills a frame of another shape.

ALTER TABLE timeline ADD COLUMN aspect TEXT NOT NULL DEFAULT '16:9'
    CHECK (aspect IN ('16:9', '9:16'));

-- 'crop': the largest window of the frame's shape, at crop_x across and
-- crop_y down, in steps from 0 (left, top) to 1000 (right, bottom).
-- 'fit': the whole picture with bars. Audio items keep the defaults.
ALTER TABLE timeline_item ADD COLUMN framing TEXT NOT NULL DEFAULT 'crop'
    CHECK (framing IN ('crop', 'fit'));
ALTER TABLE timeline_item ADD COLUMN crop_x INTEGER NOT NULL DEFAULT 500
    CHECK (crop_x BETWEEN 0 AND 1000);
ALTER TABLE timeline_item ADD COLUMN crop_y INTEGER NOT NULL DEFAULT 500
    CHECK (crop_y BETWEEN 0 AND 1000);
