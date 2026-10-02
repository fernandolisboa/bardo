-- Manual publications (#29): the posts the user made by hand from a video
-- project's export, linked by their address, and the public statistics a
-- metrics sync reads for them.

-- When a start syncs publication metrics by itself: 'off', '1h', '6h',
-- '12h' or '24h'. Values this version does not know read as the default.
ALTER TABLE user_profile ADD COLUMN metrics_sync TEXT NOT NULL DEFAULT '6h';

-- One post per project and network. Linking again replaces the row. No
-- foreign key to the account or the render: the post stays on the network
-- when either is replaced.
CREATE TABLE publication (
    id            TEXT PRIMARY KEY NOT NULL,
    project_id    TEXT NOT NULL REFERENCES video_project (id) ON DELETE CASCADE,
    network       TEXT NOT NULL,
    profile_id    TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    account_id    TEXT NOT NULL,
    render_id     TEXT NOT NULL,
    -- The post's id on the network and the network's address for it.
    post_id       TEXT NOT NULL,
    url           TEXT NOT NULL,
    -- Unix time in milliseconds.
    posted_at     INTEGER NOT NULL,
    linked_at     INTEGER NOT NULL,
    checked_at    INTEGER,
    missing_since INTEGER,
    UNIQUE (project_id, network)
) STRICT;

-- A post links to one project of a profile.
CREATE UNIQUE INDEX publication_post ON publication (profile_id, network, post_id);

-- One publication's statistics at one sync. Likes and comments are NULL
-- when the owner hides them.
CREATE TABLE metrics_snapshot (
    publication_id TEXT NOT NULL REFERENCES publication (id) ON DELETE CASCADE,
    -- Unix time in milliseconds.
    taken_at       INTEGER NOT NULL,
    views          INTEGER NOT NULL CHECK (views >= 0),
    likes          INTEGER CHECK (likes >= 0),
    comments       INTEGER CHECK (comments >= 0),
    PRIMARY KEY (publication_id, taken_at)
) STRICT, WITHOUT ROWID;
