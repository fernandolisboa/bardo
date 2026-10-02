-- Video ideas for a channel's niche: proposed by Claude, ranked by the
-- decision engine, reviewed by the user.
CREATE TABLE theme (
    id             TEXT PRIMARY KEY NOT NULL,
    profile_id     TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    channel_id     TEXT NOT NULL REFERENCES channel (id) ON DELETE CASCADE,
    -- The niche label as typed; Niche::key is derived from it.
    niche          TEXT NOT NULL,
    title          TEXT NOT NULL,
    angle          TEXT NOT NULL,
    status         TEXT NOT NULL CHECK (status IN ('suggested', 'approved', 'discarded')),
    -- Unix time in milliseconds.
    suggested_at   INTEGER NOT NULL,
    position       INTEGER NOT NULL CHECK (position >= 0),
    -- The job that proposed it; no foreign key, jobs may be cleaned up.
    suggestion_job TEXT,
    -- The ranking: every column NULL until ranked, all set after. Scores
    -- are 0-100, confidences 0-1.
    fit_score               INTEGER CHECK (fit_score BETWEEN 0 AND 100),
    fit_confidence          REAL CHECK (fit_confidence BETWEEN 0 AND 1),
    trend_score             INTEGER CHECK (trend_score BETWEEN 0 AND 100),
    trend_confidence        REAL CHECK (trend_confidence BETWEEN 0 AND 1),
    competition_score       INTEGER CHECK (competition_score BETWEEN 0 AND 100),
    competition_confidence  REAL CHECK (competition_confidence BETWEEN 0 AND 1),
    ranked_by      TEXT,
    -- Unix time in milliseconds.
    ranked_at      INTEGER,
    CHECK (
        (fit_score IS NULL) = (ranked_at IS NULL)
        AND (fit_score IS NULL) = (fit_confidence IS NULL)
        AND (fit_score IS NULL) = (trend_score IS NULL)
        AND (fit_score IS NULL) = (trend_confidence IS NULL)
        AND (fit_score IS NULL) = (competition_score IS NULL)
        AND (fit_score IS NULL) = (competition_confidence IS NULL)
        AND (fit_score IS NULL) = (ranked_by IS NULL)
    )
) STRICT;

CREATE INDEX theme_channel ON theme (channel_id);

-- One video in production, started from an approved theme.
CREATE TABLE video_project (
    id          TEXT PRIMARY KEY NOT NULL,
    profile_id  TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    channel_id  TEXT NOT NULL REFERENCES channel (id) ON DELETE CASCADE,
    niche       TEXT NOT NULL,
    -- One project per theme.
    theme_id    TEXT NOT NULL UNIQUE REFERENCES theme (id),
    title       TEXT NOT NULL,
    -- Unix time in milliseconds.
    created_at  INTEGER NOT NULL
) STRICT;

CREATE INDEX video_project_channel ON video_project (channel_id);
