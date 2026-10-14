-- Posts on X are a new meter, priced one by one (X bills each post it
-- creates). The rate table is rebuilt to allow it, as for audio seconds.
ALTER TABLE cost_record ADD COLUMN posts INTEGER NOT NULL DEFAULT 0
    CHECK (posts >= 0);

CREATE TABLE rate_new (
    profile_id   TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    provider     TEXT NOT NULL,
    model        TEXT NOT NULL,
    meter        TEXT NOT NULL
                 CHECK (meter IN ('input_tokens', 'output_tokens', 'image_tokens', 'characters',
                                  'video_seconds', 'audio_seconds', 'posts')),
    price_micros INTEGER NOT NULL CHECK (price_micros >= 0),
    PRIMARY KEY (profile_id, provider, model, meter)
) STRICT, WITHOUT ROWID;

INSERT INTO rate_new (profile_id, provider, model, meter, price_micros)
SELECT profile_id, provider, model, meter, price_micros FROM rate;

DROP TABLE rate;

ALTER TABLE rate_new RENAME TO rate;
