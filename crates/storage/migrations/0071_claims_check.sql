-- The claims check of an ad script (#276): what the script states about
-- its product, each statement tied to a claim of the Brief stage or left
-- unbacked. A project keeps one check for its current script and one for
-- a regenerated script waiting for review.
CREATE TABLE claims_check (
    project_id TEXT NOT NULL REFERENCES video_project (id) ON DELETE CASCADE,
    -- 'current' or 'pending': which of the project's scripts it read.
    script     TEXT NOT NULL,
    -- The script as the check read it: a check of another text is out of
    -- date.
    text       TEXT NOT NULL,
    -- Unix times in milliseconds. When the brief it read had last
    -- changed: a brief changed since puts the check out of date.
    brief_at   INTEGER NOT NULL,
    -- The advertiser's own words as the check read them: the product's
    -- part of them changes without the brief changing.
    notes      TEXT NOT NULL,
    checked_at INTEGER NOT NULL,
    model      TEXT NOT NULL,
    -- The job that checked it, so a resumed job does not check twice.
    job_id     TEXT,
    PRIMARY KEY (project_id, script)
) STRICT, WITHOUT ROWID;

CREATE TABLE claims_check_statement (
    project_id TEXT NOT NULL,
    script     TEXT NOT NULL,
    position   INTEGER NOT NULL CHECK (position >= 0),
    -- The sentence of the checked text that states it, from 0; NULL when
    -- the checker named a sentence the text does not have.
    sentence   INTEGER CHECK (sentence >= 0),
    says       TEXT NOT NULL,
    -- The brief's claim that backs it; NULL when no claim does.
    claim_id   TEXT,
    PRIMARY KEY (project_id, script, position),
    FOREIGN KEY (project_id, script) REFERENCES claims_check (project_id, script)
        ON DELETE CASCADE ON UPDATE CASCADE
) STRICT, WITHOUT ROWID;
