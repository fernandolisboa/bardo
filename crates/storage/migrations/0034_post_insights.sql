-- Post insights (#85): what Instagram and TikTok report to the connected
-- account about its posts, beside the views, likes and comments the
-- snapshot already keeps. NULL where the network did not report the
-- number (Instagram leaves a metric out until its data arrive), and on
-- every YouTube snapshot.
ALTER TABLE metrics_snapshot ADD COLUMN shares INTEGER CHECK (shares >= 0);
ALTER TABLE metrics_snapshot ADD COLUMN saves INTEGER CHECK (saves >= 0);
ALTER TABLE metrics_snapshot ADD COLUMN reach INTEGER CHECK (reach >= 0);
ALTER TABLE metrics_snapshot ADD COLUMN interactions INTEGER CHECK (interactions >= 0);
-- Instagram's average and total watch time, in milliseconds. YouTube's
-- watch numbers stay in the owner columns of 0030, which hold YouTube
-- Analytics' report as one group.
ALTER TABLE metrics_snapshot ADD COLUMN average_watch_ms INTEGER CHECK (average_watch_ms >= 0);
ALTER TABLE metrics_snapshot ADD COLUMN watch_time_ms INTEGER CHECK (watch_time_ms >= 0);

-- The id Instagram's insights take for a linked post (its media id), which
-- the link's shortcode is not: found once by a sync, then kept. An
-- uploaded Reel keeps its own in upload_network_id.
ALTER TABLE publication ADD COLUMN insights_id TEXT;
