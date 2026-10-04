-- TikTok drafts (#84): an upload to TikTok lands in the creator's inbox as a
-- draft, which they finish and post in the TikTok app. It gets the
-- 'draft_sent' status, with no post id or address until they link the post.
-- Rebuilt as in 0029 to widen the status check, keeping the columns 0031 and
-- 0032 added; the snapshots that refer to publications keep their rows.
CREATE TABLE publication_new (
    id                TEXT PRIMARY KEY NOT NULL,
    project_id        TEXT NOT NULL REFERENCES video_project (id) ON DELETE CASCADE,
    network           TEXT NOT NULL,
    profile_id        TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    account_id        TEXT NOT NULL,
    render_id         TEXT NOT NULL,
    kind              TEXT NOT NULL CHECK (kind IN ('manual', 'uploaded')),
    -- The post's id on the network and the network's address for it.
    post_id           TEXT,
    url               TEXT,
    -- For uploads: 'queued', 'uploading', 'processing', 'scheduled',
    -- 'draft_sent', 'published', 'restricted' or 'failed', why it failed,
    -- the visibility asked for, the publish time when scheduled (Unix time
    -- in milliseconds) and the job that sends it.
    upload_status     TEXT CHECK (upload_status IN ('queued', 'uploading', 'processing',
                                                    'scheduled', 'draft_sent', 'published',
                                                    'restricted', 'failed')),
    upload_failure    TEXT,
    upload_visibility TEXT CHECK (upload_visibility IN ('public', 'unlisted', 'private')),
    upload_publish_at INTEGER,
    upload_job        TEXT,
    -- Unix time in milliseconds.
    posted_at         INTEGER NOT NULL,
    linked_at         INTEGER NOT NULL,
    checked_at        INTEGER,
    missing_since     INTEGER,
    upload_issue      TEXT
        CHECK (upload_issue IS NULL OR (kind = 'uploaded' AND length(upload_issue) > 0)),
    upload_network_id TEXT
        CHECK (upload_network_id IS NULL
               OR (kind = 'uploaded' AND length(upload_network_id) > 0)),
    upload_claimed_at INTEGER
        CHECK (upload_claimed_at IS NULL
               OR (kind = 'uploaded' AND upload_publish_at IS NOT NULL)),
    UNIQUE (project_id, network),
    CHECK ((post_id IS NULL) = (url IS NULL)),
    CHECK (
        (kind = 'manual' AND post_id IS NOT NULL AND upload_status IS NULL
            AND upload_failure IS NULL AND upload_visibility IS NULL
            AND upload_publish_at IS NULL AND upload_job IS NULL)
        OR (kind = 'uploaded' AND upload_status IS NOT NULL
            AND upload_visibility IS NOT NULL AND upload_job IS NOT NULL
            AND (upload_failure IS NOT NULL) = (upload_status = 'failed')
            AND (upload_status <> 'scheduled' OR upload_publish_at IS NOT NULL)
            AND (upload_status <> 'draft_sent' OR post_id IS NULL))
    )
) STRICT;

INSERT INTO publication_new
    (id, project_id, network, profile_id, account_id, render_id, kind, post_id, url,
     upload_status, upload_failure, upload_visibility, upload_publish_at, upload_job,
     posted_at, linked_at, checked_at, missing_since,
     upload_issue, upload_network_id, upload_claimed_at)
SELECT id, project_id, network, profile_id, account_id, render_id, kind, post_id, url,
       upload_status, upload_failure, upload_visibility, upload_publish_at, upload_job,
       posted_at, linked_at, checked_at, missing_since,
       upload_issue, upload_network_id, upload_claimed_at
FROM publication;

DROP TABLE publication;

ALTER TABLE publication_new RENAME TO publication;

-- A post links to one project of a profile.
CREATE UNIQUE INDEX publication_post ON publication (profile_id, network, post_id);
