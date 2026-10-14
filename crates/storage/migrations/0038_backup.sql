-- Backup and restore (#143, ADR-0010). A runner restoring a backup holds
-- the whole database: while its hold is live (`held_until`, Unix ms) no
-- other runner starts a job and the background agent stops. At most one
-- row; a hold not renewed runs out like a job lease.
CREATE TABLE runner_hold (
    id          INTEGER PRIMARY KEY CHECK (id = 1),
    held_by     TEXT NOT NULL,
    held_until  INTEGER NOT NULL
) STRICT;

-- Whether Bardo backs up the database once a week by itself. On until the
-- user turns it off.
ALTER TABLE user_profile
    ADD COLUMN auto_backup INTEGER NOT NULL DEFAULT 1
    CHECK (auto_backup IN (0, 1));
