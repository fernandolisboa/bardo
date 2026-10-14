-- YouTube API Services policies (#145, ADR-0012): the version of Bardo's
-- privacy policy the user accepted before Bardo used YouTube, and when.
-- Both NULL until they accept.
ALTER TABLE user_profile ADD COLUMN privacy_version INTEGER CHECK (privacy_version >= 1);
ALTER TABLE user_profile ADD COLUMN privacy_accepted_at INTEGER
    CHECK ((privacy_version IS NULL) = (privacy_accepted_at IS NULL));
