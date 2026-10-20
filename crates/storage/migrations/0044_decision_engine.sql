-- The profile's decision engine (#164): which engine ranks themes and
-- scores cut points. Values this version does not know read as the
-- default, `jev`.
ALTER TABLE user_profile ADD COLUMN decision_engine TEXT NOT NULL DEFAULT 'jev';
