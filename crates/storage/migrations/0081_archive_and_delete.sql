-- Archive and delete (issue #336, ADR-0047 item 7). An archived video is
-- hidden from the Videos list and runs no job until it is brought back;
-- `archived_at` is when it was archived, NULL while it is not.
ALTER TABLE video_project ADD COLUMN archived_at INTEGER;

-- What the user deleted, as Costs goes on naming it: the deleted project's
-- or video's id (no foreign key: its records are gone), its name until the
-- user forgets it, the project a video belonged to, and when it went.
CREATE TABLE deleted_work (
    id TEXT PRIMARY KEY NOT NULL,
    profile_id TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('project', 'video')),
    name TEXT,
    owner_project_id TEXT,
    deleted_at INTEGER NOT NULL
);

CREATE INDEX deleted_work_profile ON deleted_work (profile_id, deleted_at);
