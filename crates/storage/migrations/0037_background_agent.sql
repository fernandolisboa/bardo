-- Background publishing agent (#87). The app and the agent run jobs from
-- this file in two processes: a runner leases a job before running it
-- (`leased_by` is the runner's id, until `leased_until`, Unix ms), and says
-- it is up in `job_runner` every few seconds.
ALTER TABLE job ADD COLUMN leased_by TEXT;
ALTER TABLE job ADD COLUMN leased_until INTEGER;

CREATE TABLE job_runner (
    id TEXT PRIMARY KEY,
    role TEXT NOT NULL CHECK (role IN ('app', 'agent')),
    started_at INTEGER NOT NULL,
    seen_at INTEGER NOT NULL
);

-- Whether the agent publishes scheduled posts while Bardo is closed. Off
-- until the user turns it on.
ALTER TABLE user_profile
    ADD COLUMN background_agent INTEGER NOT NULL DEFAULT 0
    CHECK (background_agent IN (0, 1));
