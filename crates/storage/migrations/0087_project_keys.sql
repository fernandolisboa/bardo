-- A project's own API keys and budgets (issue #343, docs/spec/projects.md
-- "Keys and budgets per project"). The keys themselves live in Windows
-- Credential Manager beside the app's; this only notes that a project has
-- one, so jobs, deletion and the restore checklist know where to look.

CREATE TABLE project_key (
    project_id TEXT NOT NULL REFERENCES project (id) ON DELETE CASCADE,
    provider   TEXT NOT NULL,
    -- Unix time in milliseconds.
    saved_at   INTEGER NOT NULL,
    PRIMARY KEY (project_id, provider)
) STRICT, WITHOUT ROWID;

-- What a project may spend in a month with its own key for a provider.
CREATE TABLE project_budget (
    project_id     TEXT NOT NULL REFERENCES project (id) ON DELETE CASCADE,
    provider       TEXT NOT NULL,
    monthly_micros INTEGER NOT NULL CHECK (monthly_micros >= 0),
    PRIMARY KEY (project_id, provider)
) STRICT, WITHOUT ROWID;

-- The project whose own key paid; NULL for the app's key. Kept when the
-- project is deleted, like the rest of its costs.
ALTER TABLE cost_record ADD COLUMN key_project_id TEXT;
