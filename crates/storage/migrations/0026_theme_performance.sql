-- Past performance in theme ranking (#33): the fourth reason of a ranking,
-- how the idea would do next to the channel's own published videos, with
-- the numbers the engine saw. Every column NULL when the channel had no
-- history at ranking time (and on every ranking made before this
-- migration), all set otherwise; the last column's check holds that rule,
-- since a column's check can only name the columns before it.

-- The engine's 0-100 score and how sure it was, 0-1.
ALTER TABLE theme ADD COLUMN performance_score INTEGER
    CHECK (performance_score BETWEEN 0 AND 100);
ALTER TABLE theme ADD COLUMN performance_confidence REAL
    CHECK (performance_confidence BETWEEN 0 AND 1);
-- What the figure averages: 'niche' (the channel's videos in the theme's
-- niche) or 'channel' (all of them, when the niche has none).
ALTER TABLE theme ADD COLUMN performance_scope TEXT
    CHECK (performance_scope IN ('niche', 'channel'));
-- The average first-week views of the videos in scope.
ALTER TABLE theme ADD COLUMN performance_views INTEGER
    CHECK (performance_views >= 0);
-- Videos in scope, and how many of them were projected from younger posts.
ALTER TABLE theme ADD COLUMN performance_videos INTEGER
    CHECK (performance_videos >= 1);
ALTER TABLE theme ADD COLUMN performance_projected INTEGER
    CHECK (performance_projected BETWEEN 0 AND performance_videos);
-- Every video of the channel the engine read.
ALTER TABLE theme ADD COLUMN performance_basis INTEGER
    CHECK (
        (performance_basis IS NULL OR performance_basis >= performance_videos)
        AND (performance_score IS NULL) = (performance_basis IS NULL)
        AND (performance_score IS NULL) = (performance_confidence IS NULL)
        AND (performance_score IS NULL) = (performance_scope IS NULL)
        AND (performance_score IS NULL) = (performance_views IS NULL)
        AND (performance_score IS NULL) = (performance_videos IS NULL)
        AND (performance_score IS NULL) = (performance_projected IS NULL)
        AND (performance_score IS NULL OR ranked_at IS NOT NULL)
    );
