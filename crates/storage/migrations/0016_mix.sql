-- The audio mix of a saved cut (#22): each audio item's fades, each audio
-- lane's level, mute and solo, and the music's ducking under the narration.

-- Fades in nanoseconds, as the user set them (an item shorter than its
-- fades plays them cut short).
ALTER TABLE timeline_item ADD COLUMN fade_in_ns INTEGER NOT NULL DEFAULT 0
    CHECK (fade_in_ns >= 0);
ALTER TABLE timeline_item ADD COLUMN fade_out_ns INTEGER NOT NULL DEFAULT 0
    CHECK (fade_out_ns >= 0);

-- Ducking on or off, and how deep, in tenths of a decibel.
ALTER TABLE timeline ADD COLUMN duck_music INTEGER NOT NULL DEFAULT 1
    CHECK (duck_music IN (0, 1));
ALTER TABLE timeline ADD COLUMN duck_depth_tenths INTEGER NOT NULL DEFAULT 120
    CHECK (duck_depth_tenths > 0);

-- One row per audio lane whose mix the cut keeps; a lane without one plays
-- at 0 dB, neither muted nor soloed.
CREATE TABLE timeline_lane (
    project_id  TEXT NOT NULL REFERENCES timeline (project_id) ON DELETE CASCADE,
    lane        TEXT NOT NULL CHECK (lane IN ('narration', 'music', 'sfx')),
    gain_tenths INTEGER NOT NULL,
    muted       INTEGER NOT NULL CHECK (muted IN (0, 1)),
    solo        INTEGER NOT NULL CHECK (solo IN (0, 1)),
    PRIMARY KEY (project_id, lane)
) STRICT, WITHOUT ROWID;
