-- Video clips: seconds of video are a new meter, priced per second. SQLite
-- cannot change a CHECK in place, so the rate table is rebuilt.
ALTER TABLE cost_record ADD COLUMN video_seconds INTEGER NOT NULL DEFAULT 0
    CHECK (video_seconds >= 0);

CREATE TABLE rate_new (
    profile_id   TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    provider     TEXT NOT NULL,
    model        TEXT NOT NULL,
    meter        TEXT NOT NULL
                 CHECK (meter IN ('input_tokens', 'output_tokens', 'image_tokens', 'characters',
                                  'video_seconds')),
    price_micros INTEGER NOT NULL CHECK (price_micros >= 0),
    PRIMARY KEY (profile_id, provider, model, meter)
) STRICT, WITHOUT ROWID;

INSERT INTO rate_new (profile_id, provider, model, meter, price_micros)
SELECT profile_id, provider, model, meter, price_micros FROM rate;

DROP TABLE rate;

ALTER TABLE rate_new RENAME TO rate;

-- The video model a channel's clips use unless a scene picks another; both
-- NULL for the app's default.
ALTER TABLE channel ADD COLUMN clip_provider TEXT;
ALTER TABLE channel ADD COLUMN clip_model TEXT
    CHECK ((clip_provider IS NULL) = (clip_model IS NULL));

-- Per scene: the user's motion prompt (NULL follows the image prompt), the
-- video model picked over the channel's, and why the last clip failed.
ALTER TABLE scene ADD COLUMN motion_prompt TEXT;
ALTER TABLE scene ADD COLUMN clip_provider TEXT;
ALTER TABLE scene ADD COLUMN clip_model TEXT
    CHECK ((clip_provider IS NULL) = (clip_model IS NULL));
ALTER TABLE scene ADD COLUMN clip_failure TEXT;

-- A scene's clips: the one the cut uses ('current') and a new one waiting
-- for review ('pending'), each a file in the project folder with its
-- length, the image generation it animates and its own generation.
CREATE TABLE scene_clip (
    plan_id         TEXT NOT NULL,
    position        INTEGER NOT NULL,
    slot            TEXT NOT NULL CHECK (slot IN ('current', 'pending')),
    file            TEXT NOT NULL,
    seconds         INTEGER NOT NULL CHECK (seconds > 0),
    source_image_id TEXT NOT NULL REFERENCES generation (id),
    generation_id   TEXT NOT NULL REFERENCES generation (id),
    PRIMARY KEY (plan_id, position, slot),
    FOREIGN KEY (plan_id, position) REFERENCES scene (plan_id, position) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;
