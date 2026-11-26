-- Projects (issue #333, ADR-0047): what the user opens to work, one at a
-- time. A project owns its channels, and through them their themes and
-- videos. Its folder stays empty until project folders (#335).
CREATE TABLE project (
    id                TEXT PRIMARY KEY NOT NULL,
    profile_id        TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    name              TEXT NOT NULL,
    color             TEXT NOT NULL,
    state             TEXT NOT NULL DEFAULT 'active' CHECK (state IN ('active', 'archived')),
    folder            TEXT,
    -- Its place among the starred projects, from 0; NULL when not starred.
    favorite_position INTEGER CHECK (favorite_position >= 0),
    -- Unix time in milliseconds; NULL until the user opens it.
    opened_at         INTEGER,
    created_at        INTEGER NOT NULL
) STRICT;

-- Backstop for the app's own check; NOCASE folds ASCII only.
CREATE UNIQUE INDEX project_profile_name ON project (profile_id, name COLLATE NOCASE);

-- Channels linked by a localized version, in either direction, share one
-- project; every other channel gets one of its own. Each group's project
-- takes the id, name and creation time of its oldest channel.
CREATE TEMP TABLE channel_group AS
WITH RECURSIVE
    version_link (a, b) AS (
        SELECT v.channel_id, s.channel_id
        FROM localization l
        JOIN video_project v ON v.id = l.project_id
        JOIN video_project s ON s.id = l.source_project_id
        WHERE v.channel_id <> s.channel_id
    ),
    link (a, b) AS (
        SELECT a, b FROM version_link
        UNION
        SELECT b, a FROM version_link
    ),
    reach (start, channel_id) AS (
        SELECT id, id FROM channel
        UNION
        SELECT reach.start, link.b FROM reach JOIN link ON link.a = reach.channel_id
    ),
    ranked (start, channel_id, n) AS (
        SELECT reach.start, c.id,
               ROW_NUMBER() OVER (PARTITION BY reach.start ORDER BY c.created_at, c.id)
        FROM reach JOIN channel c ON c.id = reach.channel_id
    )
SELECT start AS channel_id, channel_id AS project_id FROM ranked WHERE n = 1;

INSERT INTO project (id, profile_id, name, color, created_at)
SELECT c.id, c.profile_id, c.name,
       CASE (ROW_NUMBER() OVER (PARTITION BY c.profile_id ORDER BY c.created_at, c.id) - 1) % 8
           WHEN 0 THEN 'amber'
           WHEN 1 THEN 'teal'
           WHEN 2 THEN 'violet'
           WHEN 3 THEN 'rose'
           WHEN 4 THEN 'sky'
           WHEN 5 THEN 'lime'
           WHEN 6 THEN 'orange'
           ELSE 'slate'
       END,
       COALESCE(CAST(ROUND((julianday(c.created_at) - 2440587.5) * 86400000) AS INTEGER),
                CAST(ROUND((julianday('now') - 2440587.5) * 86400000) AS INTEGER))
FROM channel c
WHERE c.id IN (SELECT project_id FROM temp.channel_group);

-- SQLite cannot add a required reference in place, so the channel table
-- is rebuilt with its project; everything that points at a channel keeps
-- pointing at it by id.
CREATE TABLE channel_new (
    id                   TEXT PRIMARY KEY NOT NULL,
    profile_id           TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    name                 TEXT NOT NULL,
    niche                TEXT NOT NULL,
    aesthetic_notes      TEXT NOT NULL,
    language             TEXT NOT NULL,
    country              TEXT NOT NULL,
    created_at           TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at           TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    default_persona_id   TEXT REFERENCES persona (id) ON DELETE SET NULL,
    clip_provider        TEXT,
    clip_model           TEXT CHECK ((clip_provider IS NULL) = (clip_model IS NULL)),
    caption_style        TEXT NOT NULL DEFAULT 'clean'
                         CHECK (caption_style IN ('clean', 'boxed', 'punch')),
    transition           TEXT NOT NULL DEFAULT 'crossfade',
    transition_frames    INTEGER NOT NULL DEFAULT 15 CHECK (transition_frames > 0),
    video_format         TEXT NOT NULL DEFAULT 'long',
    motion               TEXT NOT NULL DEFAULT 'none',
    motion_strength      TEXT NOT NULL DEFAULT 'medium',
    caption_highlight    TEXT NOT NULL DEFAULT 'off',
    thumbnail_look       TEXT NOT NULL DEFAULT 'bold',
    take_provider        TEXT,
    take_model           TEXT CHECK ((take_provider IS NULL) = (take_model IS NULL)),
    production_mode      TEXT NOT NULL DEFAULT 'channel-model',
    visual_style         TEXT NOT NULL DEFAULT 'notes',
    clip_fit             TEXT NOT NULL DEFAULT 'stretch',
    clip_stretch_percent INTEGER NOT NULL DEFAULT 40
                         CHECK (clip_stretch_percent BETWEEN 5 AND 100),
    owner_project_id     TEXT NOT NULL REFERENCES project (id)
) STRICT;

INSERT INTO channel_new (id, profile_id, name, niche, aesthetic_notes, language, country,
    created_at, updated_at, default_persona_id, clip_provider, clip_model, caption_style,
    transition, transition_frames, video_format, motion, motion_strength, caption_highlight,
    thumbnail_look, take_provider, take_model, production_mode, visual_style, clip_fit,
    clip_stretch_percent, owner_project_id)
SELECT c.id, c.profile_id, c.name, c.niche, c.aesthetic_notes, c.language, c.country,
    c.created_at, c.updated_at, c.default_persona_id, c.clip_provider, c.clip_model,
    c.caption_style, c.transition, c.transition_frames, c.video_format, c.motion,
    c.motion_strength, c.caption_highlight, c.thumbnail_look, c.take_provider, c.take_model,
    c.production_mode, c.visual_style, c.clip_fit, c.clip_stretch_percent, g.project_id
FROM channel c
JOIN temp.channel_group g ON g.channel_id = c.id;

DROP TABLE channel;

ALTER TABLE channel_new RENAME TO channel;

CREATE UNIQUE INDEX channel_profile_name ON channel (profile_id, name COLLATE NOCASE);
CREATE INDEX channel_project ON channel (owner_project_id);

DROP TABLE temp.channel_group;

-- Jobs and cost records stay the profile's, and remember the project,
-- channel and video they were for, with the names they had then, so Jobs
-- and Costs group and filter by project and keep names after a rename or
-- a deletion. No foreign key: they outlive what they were for.
ALTER TABLE job ADD COLUMN owner_project_id TEXT;
ALTER TABLE job ADD COLUMN project_name TEXT;
ALTER TABLE job ADD COLUMN channel_name TEXT;
ALTER TABLE job ADD COLUMN video_title TEXT;

UPDATE job SET (owner_project_id, project_name, channel_name, video_title) = (
    SELECT p.id, p.name, c.name, v.title
    FROM channel c
    JOIN project p ON p.id = c.owner_project_id
    LEFT JOIN video_project v
        ON v.id = CASE WHEN json_valid(job.payload)
                       THEN json_extract(job.payload, '$.project') END
    WHERE c.id = COALESCE(v.channel_id,
                          CASE WHEN json_valid(job.payload)
                               THEN json_extract(job.payload, '$.channel') END)
);

CREATE INDEX job_owner_project ON job (owner_project_id);

ALTER TABLE cost_record ADD COLUMN owner_project_id TEXT;
ALTER TABLE cost_record ADD COLUMN project_name TEXT;
ALTER TABLE cost_record ADD COLUMN channel_name TEXT;
ALTER TABLE cost_record ADD COLUMN video_title TEXT;

UPDATE cost_record SET (owner_project_id, project_name, channel_name, video_title) = (
    SELECT p.id, p.name, c.name, v.title
    FROM channel c
    JOIN project p ON p.id = c.owner_project_id
    LEFT JOIN video_project v ON v.id = cost_record.project_id
    WHERE c.id = COALESCE(v.channel_id, cost_record.channel_id)
);

CREATE INDEX cost_record_owner_project ON cost_record (owner_project_id);
