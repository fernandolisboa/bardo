-- Video format (#179): Short or Long, a channel's default and a project's
-- own choice. Channels from before made long scripts and 16:9 cuts, so
-- they stay long; a NULL project format follows its channel. Codes are
-- checked by the domain.
ALTER TABLE channel ADD COLUMN video_format TEXT NOT NULL DEFAULT 'long';
ALTER TABLE video_project ADD COLUMN video_format TEXT;
