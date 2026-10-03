-- Uploaded publications (#77, ADR-0008): a publication is posted by hand
-- (manual) or uploaded by Bardo. An upload has a status and the visibility
-- it asked for, and gets its post's id and address once the network made
-- the video, so those may be empty. Rebuilt as in 0023; the snapshots that
-- refer to publications keep their rows.
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
    -- For uploads: 'queued', 'uploading', 'processing', 'published',
    -- 'restricted' or 'failed', why it failed, the visibility asked for
    -- and the job that sends it.
    upload_status     TEXT CHECK (upload_status IN ('queued', 'uploading', 'processing',
                                                    'published', 'restricted', 'failed')),
    upload_failure    TEXT,
    upload_visibility TEXT CHECK (upload_visibility IN ('public', 'unlisted', 'private')),
    upload_job        TEXT,
    -- Unix time in milliseconds.
    posted_at         INTEGER NOT NULL,
    linked_at         INTEGER NOT NULL,
    checked_at        INTEGER,
    missing_since     INTEGER,
    UNIQUE (project_id, network),
    CHECK ((post_id IS NULL) = (url IS NULL)),
    CHECK (
        (kind = 'manual' AND post_id IS NOT NULL AND upload_status IS NULL
            AND upload_failure IS NULL AND upload_visibility IS NULL AND upload_job IS NULL)
        OR (kind = 'uploaded' AND upload_status IS NOT NULL
            AND upload_visibility IS NOT NULL AND upload_job IS NOT NULL
            AND (upload_failure IS NOT NULL) = (upload_status = 'failed'))
    )
) STRICT;

INSERT INTO publication_new
    (id, project_id, network, profile_id, account_id, render_id, kind, post_id, url,
     posted_at, linked_at, checked_at, missing_since)
SELECT id, project_id, network, profile_id, account_id, render_id, 'manual', post_id, url,
       posted_at, linked_at, checked_at, missing_since
FROM publication;

DROP TABLE publication;

ALTER TABLE publication_new RENAME TO publication;

-- A post links to one project of a profile.
CREATE UNIQUE INDEX publication_post ON publication (profile_id, network, post_id);
