-- Project folders (issue #335, ADR-0047 item 3). The base folder new
-- projects go under, NULL for the default (Documents\Bardo). A video's
-- media lives in `<folder project's folder>\Videos\<folder_name>`; both are
-- NULL while it is still in the old projects folder
-- (`%LOCALAPPDATA%\Bardo\projects\<video id>`), until the media is moved.
-- The folder project is its channel's project unless the channel moved
-- since and its media has not followed yet.
ALTER TABLE user_profile ADD COLUMN base_folder TEXT;
ALTER TABLE video_project ADD COLUMN folder_name TEXT;
ALTER TABLE video_project ADD COLUMN folder_project_id TEXT REFERENCES project (id);
