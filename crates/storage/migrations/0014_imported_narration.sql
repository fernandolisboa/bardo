-- Imported narration (issue #32): the user records the script and imports
-- the file, and a provider times its words. Such a narration has no voice,
-- presets or billed characters, but the file's original name and the
-- provider that aligned it. SQLite cannot relax NOT NULL in place, so the
-- narration table is rebuilt; its words keep pointing at it by id.
CREATE TABLE narration_new (
    project_id        TEXT PRIMARY KEY NOT NULL
                      REFERENCES video_project (id) ON DELETE CASCADE,
    id                TEXT NOT NULL UNIQUE,
    profile_id        TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    text              TEXT NOT NULL,
    source            TEXT NOT NULL CHECK (source IN ('generated', 'imported')),
    -- Generated: the voice and presets that read it.
    voice_provider    TEXT,
    voice_id          TEXT,
    voice_name        TEXT,
    stability         INTEGER CHECK (stability BETWEEN 0 AND 100),
    similarity        INTEGER CHECK (similarity BETWEEN 0 AND 100),
    style             INTEGER CHECK (style BETWEEN 0 AND 100),
    speed             INTEGER CHECK (speed BETWEEN 70 AND 120),
    -- In the provider's unit (ElevenLabs: characters).
    billed_characters INTEGER CHECK (billed_characters >= 0),
    -- Imported: the file's name as the user had it, and the provider
    -- that timed its words.
    recording_name    TEXT,
    aligner           TEXT,
    -- The model that spoke or aligned it.
    model             TEXT NOT NULL,
    -- A file name inside the project folder.
    audio_file        TEXT NOT NULL,
    duration_ms       INTEGER NOT NULL CHECK (duration_ms >= 0),
    -- Unix time in milliseconds.
    generated_at      INTEGER NOT NULL,
    -- The job that generated or aligned it; no foreign key, jobs may be
    -- cleaned up.
    job_id            TEXT,
    CHECK (
        (source = 'generated'
            AND voice_provider IS NOT NULL AND voice_id IS NOT NULL AND voice_name IS NOT NULL
            AND stability IS NOT NULL AND similarity IS NOT NULL AND style IS NOT NULL
            AND speed IS NOT NULL AND billed_characters IS NOT NULL
            AND recording_name IS NULL AND aligner IS NULL)
        OR (source = 'imported'
            AND recording_name IS NOT NULL AND aligner IS NOT NULL
            AND voice_provider IS NULL AND voice_id IS NULL AND voice_name IS NULL
            AND stability IS NULL AND similarity IS NULL AND style IS NULL
            AND speed IS NULL AND billed_characters IS NULL)
    )
) STRICT;

INSERT INTO narration_new (project_id, id, profile_id, text, source, voice_provider, voice_id,
    voice_name, stability, similarity, style, speed, billed_characters, model, audio_file,
    duration_ms, generated_at, job_id)
SELECT project_id, id, profile_id, text, 'generated', voice_provider, voice_id, voice_name,
    stability, similarity, style, speed, billed_characters, model, audio_file, duration_ms,
    generated_at, job_id
FROM narration;

DROP TABLE narration;

ALTER TABLE narration_new RENAME TO narration;

-- Seconds of audio sent to be timed are a new meter, priced per hour. The
-- rate table is rebuilt to allow it, as for video seconds.
ALTER TABLE cost_record ADD COLUMN audio_seconds INTEGER NOT NULL DEFAULT 0
    CHECK (audio_seconds >= 0);

CREATE TABLE rate_new (
    profile_id   TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    provider     TEXT NOT NULL,
    model        TEXT NOT NULL,
    meter        TEXT NOT NULL
                 CHECK (meter IN ('input_tokens', 'output_tokens', 'image_tokens', 'characters',
                                  'video_seconds', 'audio_seconds')),
    price_micros INTEGER NOT NULL CHECK (price_micros >= 0),
    PRIMARY KEY (profile_id, provider, model, meter)
) STRICT, WITHOUT ROWID;

INSERT INTO rate_new (profile_id, provider, model, meter, price_micros)
SELECT profile_id, provider, model, meter, price_micros FROM rate;

DROP TABLE rate;

ALTER TABLE rate_new RENAME TO rate;
