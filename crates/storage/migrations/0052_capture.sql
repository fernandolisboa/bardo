-- Captures (#189): a recording of the user reading the script on camera,
-- imported with their confirmation. Its speech becomes the narration, in
-- the persona's voice or the user's own, and it is split into takes.

-- A captured narration keeps the recording's name and the provider that
-- timed it, like an imported one, and the voice it was converted into
-- (none for the user's own voice). The narration table is rebuilt to
-- allow it, as in 0014; its words keep pointing at it by id.
CREATE TABLE narration_new (
    project_id        TEXT PRIMARY KEY NOT NULL
                      REFERENCES video_project (id) ON DELETE CASCADE,
    id                TEXT NOT NULL UNIQUE,
    profile_id        TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    text              TEXT NOT NULL,
    source            TEXT NOT NULL CHECK (source IN ('generated', 'imported', 'captured')),
    -- Generated, or captured in a persona's voice: the voice and presets.
    voice_provider    TEXT,
    voice_id          TEXT,
    voice_name        TEXT,
    stability         INTEGER CHECK (stability BETWEEN 0 AND 100),
    similarity        INTEGER CHECK (similarity BETWEEN 0 AND 100),
    style             INTEGER CHECK (style BETWEEN 0 AND 100),
    speed             INTEGER CHECK (speed BETWEEN 70 AND 120),
    -- Generated: in the provider's unit (ElevenLabs: characters).
    billed_characters INTEGER CHECK (billed_characters >= 0),
    -- Imported or captured: the file's name as the user had it, and the
    -- provider that timed its words.
    recording_name    TEXT,
    aligner           TEXT,
    -- The model that spoke, aligned or voiced it.
    model             TEXT NOT NULL,
    -- A file name inside the project folder.
    audio_file        TEXT NOT NULL,
    duration_ms       INTEGER NOT NULL CHECK (duration_ms >= 0),
    -- Unix time in milliseconds.
    generated_at      INTEGER NOT NULL,
    -- The job that made it; no foreign key, jobs may be cleaned up.
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
        OR (source = 'captured'
            AND recording_name IS NOT NULL AND aligner IS NOT NULL
            AND billed_characters IS NULL
            AND ((voice_provider IS NOT NULL AND voice_id IS NOT NULL AND voice_name IS NOT NULL
                    AND stability IS NOT NULL AND similarity IS NOT NULL AND style IS NOT NULL
                    AND speed IS NOT NULL)
                OR (voice_provider IS NULL AND voice_id IS NULL AND voice_name IS NULL
                    AND stability IS NULL AND similarity IS NULL AND style IS NULL
                    AND speed IS NULL)))
    )
) STRICT;

INSERT INTO narration_new (project_id, id, profile_id, text, source, voice_provider, voice_id,
    voice_name, stability, similarity, style, speed, billed_characters, recording_name, aligner,
    model, audio_file, duration_ms, generated_at, job_id)
SELECT project_id, id, profile_id, text, source, voice_provider, voice_id, voice_name,
    stability, similarity, style, speed, billed_characters, recording_name, aligner, model,
    audio_file, duration_ms, generated_at, job_id
FROM narration;

DROP TABLE narration;

ALTER TABLE narration_new RENAME TO narration;

-- One capture per project, with the voice its speech was given: a
-- persona's voice and presets, or the user's own.
CREATE TABLE capture (
    project_id        TEXT PRIMARY KEY NOT NULL
                      REFERENCES video_project (id) ON DELETE CASCADE,
    id                TEXT NOT NULL UNIQUE,
    profile_id        TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    -- The file's name as the user had it, and its copy in the project
    -- folder.
    file_name         TEXT NOT NULL,
    video_file        TEXT NOT NULL,
    width             INTEGER NOT NULL CHECK (width > 0),
    height            INTEGER NOT NULL CHECK (height > 0),
    -- Frames per second, in thousandths.
    fps_milli         INTEGER NOT NULL CHECK (fps_milli > 0),
    duration_ms       INTEGER NOT NULL CHECK (duration_ms >= 0),
    -- The sentence the user ticked, as shown, and when (Unix ms).
    confirmation      TEXT NOT NULL,
    confirmed_at      INTEGER NOT NULL,
    -- 'persona' or 'own'.
    voice             TEXT NOT NULL CHECK (voice IN ('persona', 'own')),
    voice_provider    TEXT,
    voice_id          TEXT,
    voice_name        TEXT,
    stability         INTEGER CHECK (stability BETWEEN 0 AND 100),
    similarity        INTEGER CHECK (similarity BETWEEN 0 AND 100),
    style             INTEGER CHECK (style BETWEEN 0 AND 100),
    speed             INTEGER CHECK (speed BETWEEN 70 AND 120),
    -- The narration its speech became; no foreign key, a new narration
    -- replaces it and the capture goes with it.
    narration_id      TEXT NOT NULL,
    -- How much of the script its speech says in order, in thousandths.
    coverage          INTEGER NOT NULL CHECK (coverage BETWEEN 0 AND 1000),
    imported_at       INTEGER NOT NULL,
    job_id            TEXT,
    CHECK ((voice = 'persona') = (voice_provider IS NOT NULL AND voice_id IS NOT NULL
        AND voice_name IS NOT NULL AND stability IS NOT NULL AND similarity IS NOT NULL
        AND style IS NOT NULL AND speed IS NOT NULL))
) STRICT;

-- A capture's takes, in order: the narration's words each holds (word
-- indexes, end exclusive), its stretch of the recording in milliseconds
-- and its revision.
CREATE TABLE capture_take (
    project_id  TEXT NOT NULL REFERENCES capture (project_id) ON DELETE CASCADE,
    position    INTEGER NOT NULL CHECK (position >= 0),
    first_word  INTEGER NOT NULL CHECK (first_word >= 0),
    end_word    INTEGER NOT NULL CHECK (end_word > first_word),
    start_ms    INTEGER NOT NULL CHECK (start_ms >= 0),
    end_ms      INTEGER NOT NULL CHECK (end_ms >= start_ms),
    revision    INTEGER NOT NULL CHECK (revision >= 1),
    PRIMARY KEY (project_id, position)
) STRICT, WITHOUT ROWID;
