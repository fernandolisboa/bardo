-- Localized versions (#184, ADR-0023): a video project made again for
-- another channel's language and country, what it was made from and what
-- it carried over, where each adaptation's sentences start, and the
-- template kind that adapts. Codes are checked by the domain.

-- A version: the project it was made from (NULL once that one is gone),
-- the source's title, language, country and narration length, the scene
-- plan it carried over, and the source cut's shape, captions and ducking.
CREATE TABLE localization (
    project_id        TEXT PRIMARY KEY NOT NULL
                      REFERENCES video_project (id) ON DELETE CASCADE,
    profile_id        TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    source_project_id TEXT REFERENCES video_project (id) ON DELETE SET NULL,
    source_title      TEXT NOT NULL,
    source_language   TEXT NOT NULL,
    source_country    TEXT NOT NULL,
    source_length_ms  INTEGER NOT NULL CHECK (source_length_ms >= 0),
    -- No foreign key: planning the scenes again replaces the plan, and the
    -- scenes no longer move with the sentences.
    plan_id           TEXT NOT NULL,
    aspect            TEXT NOT NULL,
    caption_style     TEXT NOT NULL,
    caption_highlight TEXT NOT NULL,
    captions_shown    INTEGER NOT NULL CHECK (captions_shown IN (0, 1)),
    duck_music        INTEGER NOT NULL CHECK (duck_music IN (0, 1)),
    duck_depth_tenths INTEGER NOT NULL CHECK (duck_depth_tenths > 0),
    -- Unix time in milliseconds.
    created_at        INTEGER NOT NULL
) STRICT;

CREATE INDEX localization_source ON localization (source_project_id);

-- The source narration's sentences, in order, and when each was said.
CREATE TABLE localized_sentence (
    project_id TEXT NOT NULL REFERENCES localization (project_id) ON DELETE CASCADE,
    position   INTEGER NOT NULL CHECK (position >= 0),
    text       TEXT NOT NULL,
    start_ms   INTEGER NOT NULL CHECK (start_ms >= 0),
    PRIMARY KEY (project_id, position)
) STRICT, WITHOUT ROWID;

-- Per scene of the carried plan: the sentence it starts at and how the
-- source cut showed it (shown = 0 when the cut left it out; the framing,
-- transition and motion columns then keep their defaults).
CREATE TABLE localized_scene (
    project_id        TEXT NOT NULL REFERENCES localization (project_id) ON DELETE CASCADE,
    position          INTEGER NOT NULL CHECK (position >= 0),
    sentence          INTEGER NOT NULL CHECK (sentence >= 0),
    shown             INTEGER NOT NULL CHECK (shown IN (0, 1)),
    framing           TEXT NOT NULL DEFAULT 'crop',
    crop_x            INTEGER NOT NULL DEFAULT 500 CHECK (crop_x BETWEEN 0 AND 1000),
    crop_y            INTEGER NOT NULL DEFAULT 500 CHECK (crop_y BETWEEN 0 AND 1000),
    transition        TEXT,
    transition_frames INTEGER CHECK (transition_frames > 0),
    motion            TEXT,
    motion_strength   TEXT,
    PRIMARY KEY (project_id, position)
) STRICT, WITHOUT ROWID;

-- The source cut's lane levels.
CREATE TABLE localized_lane (
    project_id  TEXT NOT NULL REFERENCES localization (project_id) ON DELETE CASCADE,
    lane        TEXT NOT NULL,
    gain_tenths INTEGER NOT NULL,
    muted       INTEGER NOT NULL CHECK (muted IN (0, 1)),
    solo        INTEGER NOT NULL CHECK (solo IN (0, 1)),
    PRIMARY KEY (project_id, lane)
) STRICT, WITHOUT ROWID;

-- The source cut's music and SFX, in nanoseconds: where in its file each
-- starts, and where in the source narration it started and ended.
CREATE TABLE localized_audio (
    project_id  TEXT NOT NULL REFERENCES localization (project_id) ON DELETE CASCADE,
    track       TEXT NOT NULL,
    position    INTEGER NOT NULL CHECK (position >= 0),
    file        TEXT NOT NULL,
    start_ns    INTEGER NOT NULL CHECK (start_ns >= 0),
    at_ns       INTEGER NOT NULL CHECK (at_ns >= 0),
    until_ns    INTEGER NOT NULL CHECK (until_ns >= at_ns),
    fade_in_ns  INTEGER NOT NULL CHECK (fade_in_ns >= 0),
    fade_out_ns INTEGER NOT NULL CHECK (fade_out_ns >= 0),
    PRIMARY KEY (project_id, track, position)
) STRICT, WITHOUT ROWID;

-- Where each source sentence starts in the text an adaptation wrote, in
-- bytes, so the sentences can be found again in its narration.
CREATE TABLE adapted_sentence (
    generation_id TEXT NOT NULL REFERENCES generation (id) ON DELETE CASCADE,
    position      INTEGER NOT NULL CHECK (position >= 0),
    text_start    INTEGER NOT NULL CHECK (text_start >= 0),
    PRIMARY KEY (generation_id, position)
) STRICT, WITHOUT ROWID;

-- Templates of a new kind. Rebuilt as in 0023.
CREATE TABLE template_version_new (
    id           TEXT PRIMARY KEY NOT NULL,
    profile_id   TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    kind         TEXT NOT NULL
                 CHECK (kind IN ('script', 'image_prompt', 'music_prompt', 'metadata',
                                 'thumbnail', 'chapters', 'localization')),
    number       INTEGER NOT NULL CHECK (number >= 1),
    instructions TEXT NOT NULL,
    prompt       TEXT NOT NULL,
    -- Unix time in milliseconds.
    created_at   INTEGER NOT NULL,
    UNIQUE (profile_id, kind, number)
) STRICT;

INSERT INTO template_version_new
    (id, profile_id, kind, number, instructions, prompt, created_at)
SELECT id, profile_id, kind, number, instructions, prompt, created_at
FROM template_version;

DROP TABLE template_version;

ALTER TABLE template_version_new RENAME TO template_version;
