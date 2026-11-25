-- A paid call a job's run sent and had no answer to yet (#289, ADR-0030):
-- a run cut short then, by a close or a crash, leaves it, and the next run
-- fails the job instead of paying again. The app's own format; NULL when
-- no call waits for its answer.
ALTER TABLE job ADD COLUMN paid_call TEXT;
