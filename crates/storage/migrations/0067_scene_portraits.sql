-- No paid scene image without its people's portraits (#269).

-- Per image: the cast members it was drawn showing, as
-- "position:portrait generation id" pairs separated by commas, with "-" in
-- place of the id for a member drawn from the description alone. NULL for
-- an image from before this was recorded.
ALTER TABLE scene ADD COLUMN image_portraits TEXT;
ALTER TABLE scene ADD COLUMN pending_portraits TEXT;

-- The user chose to draw this person from the description alone: no
-- portrait is drawn or sent for them.
ALTER TABLE scene_cast ADD COLUMN described_only INTEGER NOT NULL DEFAULT 0;
