-- Ad angles and the brief of an ad project (#275).
--
-- An ad project is a video project that keeps its product. The app
-- refuses to delete a product while a project names it; the reference
-- is the backstop.
ALTER TABLE video_project ADD COLUMN product_id TEXT REFERENCES product (id);

CREATE INDEX video_project_product ON video_project (product_id)
    WHERE product_id IS NOT NULL;

-- One way an ad for a product could speak to a channel's audience, as
-- Claude proposed it and the user left it. Approving one starts a theme
-- and an ad project; what a theme does not hold stays here. Codes and
-- limits are checked by the domain.
CREATE TABLE ad_angle (
    id                 TEXT PRIMARY KEY NOT NULL,
    profile_id         TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    product_id         TEXT NOT NULL REFERENCES product (id) ON DELETE CASCADE,
    channel_id         TEXT NOT NULL REFERENCES channel (id) ON DELETE CASCADE,
    title              TEXT NOT NULL,
    angle              TEXT NOT NULL,
    -- An ad framework's code ('problem-solution', 'before-after', ...).
    framework          TEXT NOT NULL,
    -- The part of the audience it speaks to.
    segment            TEXT NOT NULL,
    -- A draft of the ad's first words.
    hook               TEXT NOT NULL,
    -- 'suggested', 'approved' or 'discarded', as for themes.
    status             TEXT NOT NULL,
    -- Unix time in milliseconds.
    suggested_at       INTEGER NOT NULL,
    position           INTEGER NOT NULL CHECK (position >= 0),
    -- The job that proposed it, so a resumed job does not propose twice.
    suggestion_job     TEXT,
    -- The ranking: three 0 to 100 scores with their confidence, all NULL
    -- until the angle is ranked.
    fit_score          INTEGER CHECK (fit_score BETWEEN 0 AND 100),
    fit_confidence     REAL,
    hook_score         INTEGER CHECK (hook_score BETWEEN 0 AND 100),
    hook_confidence    REAL,
    clarity_score      INTEGER CHECK (clarity_score BETWEEN 0 AND 100),
    clarity_confidence REAL,
    ranked_by          TEXT,
    ranked_at          INTEGER,
    -- The theme it became when approved.
    theme_id           TEXT REFERENCES theme (id) ON DELETE SET NULL
) STRICT;

CREATE INDEX ad_angle_product_channel ON ad_angle (product_id, channel_id, suggested_at);
CREATE INDEX ad_angle_theme ON ad_angle (theme_id) WHERE theme_id IS NOT NULL;

-- The claims of the product sheet an angle leans on. A claim keeps its
-- id for as long as it is on the sheet; one that left it is kept here as
-- a name the sheet no longer knows.
CREATE TABLE ad_angle_claim (
    angle_id TEXT NOT NULL REFERENCES ad_angle (id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK (position >= 0),
    claim_id TEXT NOT NULL,
    PRIMARY KEY (angle_id, position)
) STRICT, WITHOUT ROWID;

-- What one ad project says about its product: the Brief stage.
CREATE TABLE ad_brief (
    project_id     TEXT PRIMARY KEY NOT NULL REFERENCES video_project (id) ON DELETE CASCADE,
    product_id     TEXT NOT NULL REFERENCES product (id),
    angle_id       TEXT NOT NULL REFERENCES ad_angle (id) ON DELETE CASCADE,
    offer          TEXT NOT NULL,
    call_to_action TEXT NOT NULL,
    link           TEXT NOT NULL,
    -- Unix times in milliseconds. The Script stage opens once the brief
    -- is confirmed.
    confirmed_at   INTEGER,
    -- When what a script reads here last changed.
    changed_at     INTEGER NOT NULL,
    -- When the user last said the script still holds after a change.
    script_kept_at INTEGER
) STRICT;

CREATE INDEX ad_brief_product ON ad_brief (product_id);

-- The video's own copy of each claim it uses, so that editing a product
-- never changes a video silently.
CREATE TABLE ad_brief_claim (
    project_id     TEXT NOT NULL REFERENCES ad_brief (project_id) ON DELETE CASCADE,
    position       INTEGER NOT NULL CHECK (position >= 0),
    -- The product's claim it was copied from.
    claim_id       TEXT NOT NULL,
    text           TEXT NOT NULL,
    kind           TEXT NOT NULL,
    sensitive      INTEGER NOT NULL CHECK (sensitive IN (0, 1)),
    -- What the product's claim said when the user kept this copy over a
    -- change there: its text and whether it was confirmed. Both NULL
    -- until the user keeps a copy.
    kept_text      TEXT,
    kept_confirmed INTEGER CHECK (kept_confirmed IN (0, 1)),
    PRIMARY KEY (project_id, position),
    UNIQUE (project_id, claim_id)
) STRICT;

-- Templates of a new kind, and of the kinds the next ad slices bring.
-- Rebuilt as in 0055, this time without the list of kinds: the domain
-- checks the code, as it does for every table since products.
CREATE TABLE template_version_new (
    id           TEXT PRIMARY KEY NOT NULL,
    profile_id   TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    kind         TEXT NOT NULL,
    number       INTEGER NOT NULL CHECK (number >= 1),
    instructions TEXT NOT NULL,
    prompt       TEXT NOT NULL,
    -- Unix time in milliseconds.
    created_at   INTEGER NOT NULL,
    UNIQUE (profile_id, kind, number)
) STRICT;

INSERT INTO template_version_new
    (id, profile_id, kind, number, instructions, prompt, created_at)
SELECT id, profile_id, kind, number, instructions, prompt, created_at
FROM template_version;

DROP TABLE template_version;

ALTER TABLE template_version_new RENAME TO template_version;
