-- In-app scheduler (#82). A queued job may wait for its time: a schedule,
-- or something outside Bardo (a network's publishing limit). That time is
-- its own column, apart from retry_at, which stays the backoff after a
-- failed attempt. Unix time in milliseconds.
ALTER TABLE job ADD COLUMN run_at INTEGER;

-- Jobs that waited for a publishing limit kept that time in retry_at, with
-- no failure: it moves to run_at.
UPDATE job SET run_at = retry_at, retry_at = NULL
WHERE state = 'queued' AND failure_kind IS NULL AND retry_at IS NOT NULL;

-- When the run that publishes a scheduled upload at its due time took it
-- (Unix time in milliseconds), so no other run publishes it again and a
-- restart tells a run in progress from one that never started. Only an
-- upload with a due time (upload_publish_at) is claimed.
ALTER TABLE publication ADD COLUMN upload_claimed_at INTEGER
    CHECK (upload_claimed_at IS NULL
           OR (kind = 'uploaded' AND upload_publish_at IS NOT NULL));
