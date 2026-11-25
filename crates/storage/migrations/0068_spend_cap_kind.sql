-- A video's spend cap is one of: the default of its format, the user's own
-- amount (kept in spend_cap_micros), none by the user's choice, or none
-- because the project is from before videos started with a cap (#268). The
-- projects there are keep what they had: an amount stays the user's own,
-- and one without a cap is offered the default once.
ALTER TABLE video_project ADD COLUMN spend_cap_kind TEXT NOT NULL DEFAULT 'not_offered';
UPDATE video_project SET spend_cap_kind = 'own' WHERE spend_cap_micros IS NOT NULL;
