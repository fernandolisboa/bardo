-- Video transitions (#167): the transition into a video item from the one
-- before, and each channel's default transition for new ones.

-- A cut where both are NULL. Kind codes are checked by the domain, as the
-- track names are, so a new kind needs no table rebuild.
ALTER TABLE timeline_item ADD COLUMN transition TEXT;
ALTER TABLE timeline_item ADD COLUMN transition_frames INTEGER
    CHECK (transition_frames > 0);

ALTER TABLE channel ADD COLUMN transition TEXT NOT NULL DEFAULT 'crossfade';
ALTER TABLE channel ADD COLUMN transition_frames INTEGER NOT NULL DEFAULT 15
    CHECK (transition_frames > 0);
