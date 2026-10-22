-- Motion on stills (#180): how the view moves over a video item's still
-- picture, and each channel's motions for new rough cuts.

-- No motion where both are NULL. Codes are checked by the domain, as the
-- transition kinds are, so a new motion needs no table rebuild.
ALTER TABLE timeline_item ADD COLUMN motion TEXT;
ALTER TABLE timeline_item ADD COLUMN motion_strength TEXT;

-- Channels from before keep their stills still: a rough cut nobody edited
-- is rebuilt from its channel, so motion there would change cuts and
-- outdate renders made before. New channels get varied motions from the
-- domain's default.
ALTER TABLE channel ADD COLUMN motion TEXT NOT NULL DEFAULT 'none';
ALTER TABLE channel ADD COLUMN motion_strength TEXT NOT NULL DEFAULT 'medium';
