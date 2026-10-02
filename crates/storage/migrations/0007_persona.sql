-- The user's narrator library. A persona points at a voice its provider
-- holds; it never stores voice samples, audio or credentials.
CREATE TABLE persona (
    id             TEXT PRIMARY KEY NOT NULL,
    profile_id     TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    name           TEXT NOT NULL,
    voice_provider TEXT NOT NULL,
    voice_id       TEXT NOT NULL,
    -- The voice's name when picked, to show without asking the provider.
    voice_name     TEXT NOT NULL,
    tone           TEXT NOT NULL,
    script_style   TEXT NOT NULL,
    -- Generation presets in whole percent.
    stability      INTEGER NOT NULL CHECK (stability BETWEEN 0 AND 100),
    similarity     INTEGER NOT NULL CHECK (similarity BETWEEN 0 AND 100),
    style          INTEGER NOT NULL CHECK (style BETWEEN 0 AND 100),
    speed          INTEGER NOT NULL CHECK (speed BETWEEN 70 AND 120),
    created_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

-- Backstop for the app's own check; NOCASE folds ASCII only.
CREATE UNIQUE INDEX persona_profile_name ON persona (profile_id, name COLLATE NOCASE);

-- The persona a channel's videos use unless a video overrides it.
ALTER TABLE channel ADD COLUMN default_persona_id TEXT
    REFERENCES persona (id) ON DELETE SET NULL;
