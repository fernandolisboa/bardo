-- Recurring objects of a scene plan (issue #361): a cup, a lantern, a
-- notebook the scenes keep showing. They are cast members of the plan with
-- a kind, so each one gets a reference picture drawn once and sent with
-- every scene that shows it, exactly as a person's portrait is. Members
-- saved before are people.

ALTER TABLE scene_cast ADD COLUMN kind TEXT NOT NULL DEFAULT 'person'
    CHECK (kind IN ('person', 'object'));
