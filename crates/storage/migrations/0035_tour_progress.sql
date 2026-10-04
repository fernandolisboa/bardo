-- Guided tours (#105): how far each profile got in each tour, and the
-- content version it saw. A tour whose content version rises shows as new
-- again; nothing restarts on its own. Codes this version does not know
-- (a tour removed later) are ignored on read.
CREATE TABLE tour_progress (
    profile_id      TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    tour            TEXT NOT NULL,
    content_version INTEGER NOT NULL CHECK (content_version >= 1),
    state           TEXT NOT NULL
                    CHECK (state IN ('offered', 'in_progress', 'completed', 'dismissed')),
    last_step       INTEGER NOT NULL DEFAULT 0 CHECK (last_step >= 0),
    updated_at      INTEGER NOT NULL,
    PRIMARY KEY (profile_id, tour)
) STRICT;
