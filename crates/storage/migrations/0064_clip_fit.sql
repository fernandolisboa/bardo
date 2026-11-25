-- Stretched clips (#260): a scene a little longer than a clip length a
-- model makes pays for that length, and the clip plays slowed to fit.

-- 'stretch' asks for the shorter length; 'cover' always asks for one that
-- covers the scene, which is what every clip was asked for before. Codes
-- are read by the domain; one it does not know is read as the default.
ALTER TABLE channel ADD COLUMN clip_fit TEXT NOT NULL DEFAULT 'stretch';

-- How much longer than itself a stretched clip may play, in percent: at
-- 40, a 5 second clip covers a scene of up to 7. Kept while the channel
-- covers every scene.
ALTER TABLE channel ADD COLUMN clip_stretch_percent INTEGER NOT NULL DEFAULT 40
    CHECK (clip_stretch_percent BETWEEN 5 AND 100);

-- The limit a clip was asked for shorter than its scene under, to play
-- slowed; 0 for a clip that plays at its own speed, as every clip made
-- before this does.
ALTER TABLE scene_clip ADD COLUMN stretch_percent INTEGER NOT NULL DEFAULT 0
    CHECK (stretch_percent BETWEEN 0 AND 100);
