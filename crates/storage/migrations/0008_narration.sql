-- A video project's narration: the script text it read, the voice and
-- presets that read it, what the provider billed, the audio file in the
-- project folder and the timing of every word. One per project; a new
-- narration replaces the old one.
CREATE TABLE narration (
    project_id        TEXT PRIMARY KEY NOT NULL
                      REFERENCES video_project (id) ON DELETE CASCADE,
    id                TEXT NOT NULL UNIQUE,
    profile_id        TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    text              TEXT NOT NULL,
    voice_provider    TEXT NOT NULL,
    voice_id          TEXT NOT NULL,
    voice_name        TEXT NOT NULL,
    -- Generation presets in whole percent, as the persona had them.
    stability         INTEGER NOT NULL CHECK (stability BETWEEN 0 AND 100),
    similarity        INTEGER NOT NULL CHECK (similarity BETWEEN 0 AND 100),
    style             INTEGER NOT NULL CHECK (style BETWEEN 0 AND 100),
    speed             INTEGER NOT NULL CHECK (speed BETWEEN 70 AND 120),
    model             TEXT NOT NULL,
    -- In the provider's unit (ElevenLabs: characters).
    billed_characters INTEGER NOT NULL CHECK (billed_characters >= 0),
    -- A file name inside the project folder.
    audio_file        TEXT NOT NULL,
    duration_ms       INTEGER NOT NULL CHECK (duration_ms >= 0),
    -- Unix time in milliseconds.
    generated_at      INTEGER NOT NULL,
    -- The job that generated it; no foreign key, jobs may be cleaned up.
    job_id            TEXT
) STRICT;

-- When each word of a narration's text is spoken. Words are byte ranges
-- of the narration's text, in reading order.
CREATE TABLE narration_word (
    narration_id TEXT NOT NULL REFERENCES narration (id) ON DELETE CASCADE,
    position     INTEGER NOT NULL CHECK (position >= 0),
    text_start   INTEGER NOT NULL CHECK (text_start >= 0),
    text_end     INTEGER NOT NULL CHECK (text_end > text_start),
    start_ms     INTEGER NOT NULL CHECK (start_ms >= 0),
    end_ms       INTEGER NOT NULL CHECK (end_ms >= start_ms),
    PRIMARY KEY (narration_id, position)
) STRICT, WITHOUT ROWID;
