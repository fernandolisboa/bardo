-- Screen tours (#107): whether a screen's "Tour this screen" button is
-- marked new until its tour is completed or dismissed. On by default.
ALTER TABLE user_profile
    ADD COLUMN offer_screen_tours INTEGER NOT NULL DEFAULT 1
    CHECK (offer_screen_tours IN (0, 1));
