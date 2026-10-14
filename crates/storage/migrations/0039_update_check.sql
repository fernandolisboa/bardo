-- Whether Bardo looks for a newer release when it opens and once a day
-- (#144, ADR-0011). On until the user turns it off.
ALTER TABLE user_profile
    ADD COLUMN update_check INTEGER NOT NULL DEFAULT 1
    CHECK (update_check IN (0, 1));
