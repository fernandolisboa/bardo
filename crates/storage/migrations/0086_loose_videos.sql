-- Loose videos (issue #341, ADR-0047): a video with no project and no
-- channel, made from an idea the user types, holding itself what a
-- channel would give it. `channel_id` and `theme_id` become optional, so
-- the table is rebuilt; its columns keep their order and meaning.

CREATE TABLE video_project_new (
    id                TEXT PRIMARY KEY NOT NULL,
    profile_id        TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    -- NULL for a loose video.
    channel_id        TEXT REFERENCES channel (id) ON DELETE CASCADE,
    niche             TEXT NOT NULL,
    -- One project per theme; NULL for a video that started loose.
    theme_id          TEXT UNIQUE REFERENCES theme (id),
    title             TEXT NOT NULL,
    -- Unix time in milliseconds.
    created_at        INTEGER NOT NULL,
    persona_id        TEXT REFERENCES persona (id) ON DELETE SET NULL,
    video_format      TEXT,
    take_cap_micros   INTEGER CHECK (take_cap_micros > 0),
    production_mode   TEXT,
    spend_cap_micros  INTEGER CHECK (spend_cap_micros > 0),
    video_length      TEXT,
    visual_style      TEXT,
    spend_cap_kind    TEXT NOT NULL DEFAULT 'not_offered',
    product_id        TEXT REFERENCES product (id),
    folder_name       TEXT,
    folder_project_id TEXT REFERENCES project (id),
    archived_at       INTEGER,
    -- A loose video's media: `<loose_folder>\<folder_name>`, the loose
    -- videos' folder in the global folder when it was made. NULL once it
    -- is in a project's folder.
    loose_folder      TEXT,
    -- A theme only through a channel.
    CHECK (channel_id IS NOT NULL OR theme_id IS NULL)
) STRICT;

INSERT INTO video_project_new
    (id, profile_id, channel_id, niche, theme_id, title, created_at, persona_id,
     video_format, take_cap_micros, production_mode, spend_cap_micros, video_length,
     visual_style, spend_cap_kind, product_id, folder_name, folder_project_id, archived_at)
SELECT id, profile_id, channel_id, niche, theme_id, title, created_at, persona_id,
       video_format, take_cap_micros, production_mode, spend_cap_micros, video_length,
       visual_style, spend_cap_kind, product_id, folder_name, folder_project_id, archived_at
FROM video_project;

DROP TABLE video_project;

ALTER TABLE video_project_new RENAME TO video_project;

CREATE INDEX video_project_channel ON video_project (channel_id);

CREATE INDEX video_project_product ON video_project (product_id)
    WHERE product_id IS NOT NULL;

CREATE INDEX video_project_loose ON video_project (profile_id)
    WHERE channel_id IS NULL;

-- What a video holds itself in place of its channel's settings: a loose
-- video's, kept when it moves into a project (its own settings win over
-- the channel's, except the channel's name, niche and themes). The
-- columns are the channel's, with the angle of the idea it started from.
CREATE TABLE video_settings (
    project_id           TEXT PRIMARY KEY NOT NULL
                         REFERENCES video_project (id) ON DELETE CASCADE,
    angle                TEXT NOT NULL,
    name                 TEXT NOT NULL,
    niche                TEXT NOT NULL,
    aesthetic_notes      TEXT NOT NULL,
    language             TEXT NOT NULL,
    country              TEXT NOT NULL,
    default_persona_id   TEXT REFERENCES persona (id) ON DELETE SET NULL,
    clip_provider        TEXT,
    clip_model           TEXT,
    caption_style        TEXT NOT NULL,
    transition           TEXT NOT NULL,
    transition_frames    INTEGER NOT NULL,
    video_format         TEXT NOT NULL,
    motion               TEXT NOT NULL,
    motion_strength      TEXT NOT NULL,
    caption_highlight    TEXT NOT NULL,
    thumbnail_look       TEXT NOT NULL,
    take_provider        TEXT,
    take_model           TEXT,
    production_mode      TEXT NOT NULL,
    visual_style         TEXT NOT NULL,
    clip_fit             TEXT NOT NULL,
    clip_stretch_percent INTEGER NOT NULL
) STRICT;

CREATE INDEX video_settings_persona ON video_settings (default_persona_id)
    WHERE default_persona_id IS NOT NULL;

-- When the user last opened *No project* (the loose videos); it is the
-- open choice while it is later than every project's opened_at.
ALTER TABLE user_profile ADD COLUMN loose_opened_at INTEGER;

-- A loose video renders for each network without an account (it is never
-- published), so a render's account is no longer always a saved one: the
-- table is rebuilt without that reference, and a trigger removes an
-- account's renders with it, as the reference did.
CREATE TABLE render_new (
    id                TEXT PRIMARY KEY NOT NULL,
    project_id        TEXT NOT NULL REFERENCES video_project (id) ON DELETE CASCADE,
    profile_id        TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    -- A saved network account, or a loose video's target for a network.
    account_id        TEXT NOT NULL,
    network           TEXT NOT NULL,
    aspect            TEXT NOT NULL,
    resolution        INTEGER NOT NULL CHECK (resolution > 0),
    codec             TEXT NOT NULL,
    bitrate_kbps      INTEGER NOT NULL CHECK (bitrate_kbps > 0),
    max_duration_secs INTEGER NOT NULL CHECK (max_duration_secs > 0),
    loudness_tenths   INTEGER NOT NULL,
    file              TEXT NOT NULL,
    encoder           TEXT NOT NULL,
    duration_ns       INTEGER NOT NULL CHECK (duration_ns > 0),
    size_bytes        INTEGER NOT NULL CHECK (size_bytes >= 0),
    integrated_cents  INTEGER,
    true_peak_cents   INTEGER,
    cut               TEXT NOT NULL,
    rendered_at       INTEGER NOT NULL,
    CHECK ((integrated_cents IS NULL) = (true_peak_cents IS NULL)),
    UNIQUE (project_id, account_id)
) STRICT;

INSERT INTO render_new SELECT * FROM render;

DROP TABLE render;

ALTER TABLE render_new RENAME TO render;

CREATE INDEX render_account ON render (account_id);

CREATE TRIGGER network_account_renders AFTER DELETE ON network_account
BEGIN
    DELETE FROM render WHERE account_id = OLD.id;
END;
