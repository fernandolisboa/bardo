-- A video's length (#253), instead of its format's: NULL follows the
-- format. Codes are checked by the domain, as formats are.
ALTER TABLE video_project ADD COLUMN video_length TEXT;
