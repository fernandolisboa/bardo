-- What an ad's upload or export was reviewed with (#282): its commercial
-- disclosure, the brand accounts a paid-partnership Reel names, and the
-- items the user acknowledged (the sensitive claims, the unbacked
-- statements, an out-of-date check, claims the product no longer
-- confirms, the restricted category and the concept notice). Projects
-- that are not ads have no row.
CREATE TABLE publication_ad_review (
    publication_id TEXT PRIMARY KEY REFERENCES publication (id) ON DELETE CASCADE,
    -- 'none', 'your_brand' or 'paid_partnership'.
    disclosure     TEXT NOT NULL,
    -- Instagram account ids, one per line.
    sponsors       TEXT NOT NULL,
    -- The acknowledged items' keys, one per line.
    acknowledged   TEXT NOT NULL
) STRICT, WITHOUT ROWID;

-- The same for a network's last export of an ad project.
CREATE TABLE export_ad_review (
    project_id   TEXT NOT NULL REFERENCES video_project (id) ON DELETE CASCADE,
    network      TEXT NOT NULL,
    disclosure   TEXT NOT NULL,
    acknowledged TEXT NOT NULL,
    PRIMARY KEY (project_id, network)
) STRICT, WITHOUT ROWID;
