-- Instagram Reels upload (#81): what the network said it published without
-- (Instagram's config_issue on media_publish, such as a caption or tags it
-- did not attach), in its own words. NULL when it said nothing, and always
-- for manual publications.
ALTER TABLE publication ADD COLUMN upload_issue TEXT
    CHECK (upload_issue IS NULL OR (kind = 'uploaded' AND length(upload_issue) > 0));

-- The post's id at the network once Bardo published it, when its address
-- carries another one (Instagram's media id, which its insights take; the
-- link holds the Reel's shortcode).
ALTER TABLE publication ADD COLUMN upload_network_id TEXT
    CHECK (upload_network_id IS NULL
           OR (kind = 'uploaded' AND length(upload_network_id) > 0));
