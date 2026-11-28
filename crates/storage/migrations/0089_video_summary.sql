-- Whether an open video shows its summary under its title (#366). On
-- until the user collapses it; it applies to every video.
ALTER TABLE user_profile
    ADD COLUMN video_summary INTEGER NOT NULL DEFAULT 1
    CHECK (video_summary IN (0, 1));
