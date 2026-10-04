-- Owner metrics (#79): what the channel's owner reads of a YouTube post
-- through the connected account (YouTube Analytics), kept on the same
-- snapshot as its public statistics. All NULL when the account is not
-- connected or the network had no data for the post yet.

-- The owner's view count, engaged views, minutes watched and the average
-- playback in whole seconds.
ALTER TABLE metrics_snapshot ADD COLUMN owner_views INTEGER CHECK (owner_views >= 0);
ALTER TABLE metrics_snapshot ADD COLUMN engaged_views INTEGER CHECK (engaged_views >= 0);
ALTER TABLE metrics_snapshot ADD COLUMN minutes_watched INTEGER CHECK (minutes_watched >= 0);
ALTER TABLE metrics_snapshot ADD COLUMN average_view_seconds INTEGER
    CHECK (average_view_seconds >= 0);
-- How much of the video an average playback covers, in ten-thousandths.
ALTER TABLE metrics_snapshot ADD COLUMN average_view_share INTEGER
    CHECK (average_view_share >= 0);
-- For a monetized channel: revenue, CPM and playback-based CPM in
-- millionths of a US dollar.
ALTER TABLE metrics_snapshot ADD COLUMN revenue_micros INTEGER CHECK (revenue_micros >= 0);
ALTER TABLE metrics_snapshot ADD COLUMN cpm_micros INTEGER CHECK (cpm_micros >= 0);
ALTER TABLE metrics_snapshot ADD COLUMN playback_cpm_micros INTEGER
    CHECK (playback_cpm_micros >= 0);
-- 1 for a monetized channel, 0 outside the Partner Program. The group
-- check sits on this last column: a column check cannot name later ones.
ALTER TABLE metrics_snapshot ADD COLUMN monetized INTEGER CHECK (
    (monetized IS NULL AND owner_views IS NULL AND engaged_views IS NULL
        AND minutes_watched IS NULL AND average_view_seconds IS NULL
        AND average_view_share IS NULL AND revenue_micros IS NULL
        AND cpm_micros IS NULL AND playback_cpm_micros IS NULL)
    OR (owner_views IS NOT NULL AND engaged_views IS NOT NULL
        AND minutes_watched IS NOT NULL AND average_view_seconds IS NOT NULL
        AND average_view_share IS NOT NULL
        AND ((monetized IS 1 AND revenue_micros IS NOT NULL AND cpm_micros IS NOT NULL
                AND playback_cpm_micros IS NOT NULL)
            OR (monetized IS 0 AND revenue_micros IS NULL AND cpm_micros IS NULL
                AND playback_cpm_micros IS NULL)))
);

-- A post's retention curve as the last sync read it: at each moment of the
-- video (elapsed, 0 to 10000 ten-thousandths), the share still watching
-- (above 10000 where parts are rewatched) and how that compares with
-- videos of similar length (0 to 10000; NULL when not reported).
CREATE TABLE retention_point (
    publication_id TEXT NOT NULL REFERENCES publication (id) ON DELETE CASCADE,
    elapsed        INTEGER NOT NULL CHECK (elapsed BETWEEN 0 AND 10000),
    watch          INTEGER NOT NULL CHECK (watch >= 0),
    relative       INTEGER CHECK (relative BETWEEN 0 AND 10000),
    -- Unix time in milliseconds.
    read_at        INTEGER NOT NULL,
    PRIMARY KEY (publication_id, elapsed)
) STRICT, WITHOUT ROWID;
