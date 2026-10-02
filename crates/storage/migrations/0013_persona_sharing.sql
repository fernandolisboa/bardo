-- Persona sharing (issue #31). An imported persona points at a voice in
-- someone else's provider account; until the user's own account is seen
-- to have it, the persona is flagged and cannot narrate. NULL means not
-- flagged.
ALTER TABLE persona ADD COLUMN voice_flag TEXT
    CHECK (voice_flag IN ('unchecked', 'unavailable'));

-- The persona that narrates a video instead of its channel's default;
-- NULL follows the channel.
ALTER TABLE video_project ADD COLUMN persona_id TEXT
    REFERENCES persona (id) ON DELETE SET NULL;
