-- Production modes (#248): how a channel's videos are produced, which a
-- video may override, and what a video may cost before Bardo asks.

-- Channels from before animate every scene with their clip model, as they
-- did; new channels get the domain's default (the animated hook).
ALTER TABLE channel ADD COLUMN production_mode TEXT NOT NULL DEFAULT 'channel-model';

-- NULL follows the channel. Codes are checked by the domain, as formats are.
ALTER TABLE video_project ADD COLUMN production_mode TEXT;

-- NULL has no cap.
ALTER TABLE video_project ADD COLUMN spend_cap_micros INTEGER CHECK (spend_cap_micros > 0);
