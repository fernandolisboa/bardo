-- Visual styles (#259): how a channel's videos are rendered, which a video
-- may override.

-- 'notes' takes the look from the channel's aesthetic notes, as every
-- channel did before styles. Codes are read by the domain; one it does not
-- know is read as 'notes'.
ALTER TABLE channel ADD COLUMN visual_style TEXT NOT NULL DEFAULT 'notes';

-- NULL follows the channel.
ALTER TABLE video_project ADD COLUMN visual_style TEXT;
