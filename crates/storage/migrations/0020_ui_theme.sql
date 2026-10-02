-- The profile's interface theme preference (issue #59): `system:<light>:<dark>`
-- or `fixed:<theme>`. Values this version does not know read as the default.
ALTER TABLE user_profile ADD COLUMN ui_theme TEXT NOT NULL DEFAULT 'system:paper:graphite';
