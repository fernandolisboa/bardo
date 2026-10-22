-- Motion on stills (#180): how the view moves over a video item's still
-- picture, and each channel's motions for new rough cuts.

-- No motion where both are NULL. Codes are checked by the domain, as the
-- transition kinds are, so a new motion needs no table rebuild.
ALTER TABLE timeline_item ADD COLUMN motion TEXT;
ALTER TABLE timeline_item ADD COLUMN motion_strength TEXT;

-- Every channel, old ones too, varies its new cuts' motions: cuts already
-- saved keep their items' NULLs.
ALTER TABLE channel ADD COLUMN motion TEXT NOT NULL DEFAULT 'varied';
ALTER TABLE channel ADD COLUMN motion_strength TEXT NOT NULL DEFAULT 'medium';
