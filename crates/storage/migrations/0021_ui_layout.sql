-- The profile's layout (issue #61): where every screen's parts go. Values
-- this version does not know read as the default, `workspace`.
ALTER TABLE user_profile ADD COLUMN ui_layout TEXT NOT NULL DEFAULT 'workspace';
