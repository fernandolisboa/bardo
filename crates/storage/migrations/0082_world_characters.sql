-- World: characters picked per video, reference pictures kept per look
-- (issue #337, ADR-0047 item 6).

-- A character of a project's world, or of the global library when
-- `owner_project_id` is NULL. Its name is unique per owner, ignoring case.
-- Deleting the project deletes its characters.
CREATE TABLE character (
    id               TEXT PRIMARY KEY NOT NULL,
    profile_id       TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    owner_project_id TEXT REFERENCES project (id) ON DELETE CASCADE,
    name             TEXT NOT NULL,
    appearance       TEXT NOT NULL,
    always_note      TEXT NOT NULL DEFAULT '',
    never_note       TEXT NOT NULL DEFAULT '',
    -- Unix time in milliseconds; NULL while active.
    archived_at      INTEGER,
    created_at       INTEGER NOT NULL
) STRICT;

CREATE UNIQUE INDEX character_name
    ON character (profile_id, IFNULL(owner_project_id, ''), name COLLATE NOCASE);

-- A character's portrait for one look: a catalog style's code, or 'notes'
-- with the channel and the notes' text it was drawn for. Its provenance is
-- its own (no generation row, which belongs to a video): deleting the video
-- that first drew it leaves it. `kept` is 0 while one drawn from the World
-- screen waits for review. The file is in the owner's World folder.
CREATE TABLE reference_picture (
    id            TEXT PRIMARY KEY NOT NULL,
    character_id  TEXT NOT NULL REFERENCES character (id) ON DELETE CASCADE,
    look          TEXT NOT NULL,
    look_channel  TEXT,
    look_notes    TEXT,
    file          TEXT NOT NULL,
    provider      TEXT NOT NULL,
    model         TEXT NOT NULL,
    prompt        TEXT NOT NULL,
    origin        TEXT NOT NULL,
    generated_at  INTEGER NOT NULL,
    -- No foreign key: jobs may be cleaned up.
    job_id        TEXT,
    kept          INTEGER NOT NULL DEFAULT 1 CHECK (kept IN (0, 1))
) STRICT;

CREATE UNIQUE INDEX reference_picture_look
    ON reference_picture (character_id, look, IFNULL(look_channel, ''),
                          IFNULL(look_notes, ''), kept);

-- Who appears: the characters a video picked, in order, each with the
-- video's own instruction.
CREATE TABLE video_cast_pick (
    project_id   TEXT NOT NULL REFERENCES video_project (id) ON DELETE CASCADE,
    character_id TEXT NOT NULL REFERENCES character (id) ON DELETE CASCADE,
    position     INTEGER NOT NULL CHECK (position >= 0),
    instruction  TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (project_id, character_id)
) STRICT, WITHOUT ROWID;

CREATE INDEX video_cast_pick_character ON video_cast_pick (character_id);

-- A cast member that is a world character; NULL for an extra of the video.
-- No foreign key, so the column can be added in place: deleting a
-- character clears it, and a write names only a character that exists.
ALTER TABLE scene_cast ADD COLUMN character_id TEXT;

-- Who appears changed after the plan was made: the plan waits for review.
-- A new plan is a new row, which starts at 0.
ALTER TABLE scene_plan ADD COLUMN picks_changed INTEGER NOT NULL DEFAULT 0;
