-- Product research and the product sheet (#274).
--
-- A research run searches the web, and its request asks for the provider's
-- prompt cache so that pages already read are not billed in full at every
-- step. Searches and the two kinds of cached tokens are new meters; the
-- rate table is rebuilt to allow them, as for posts.
ALTER TABLE cost_record ADD COLUMN searches INTEGER NOT NULL DEFAULT 0
    CHECK (searches >= 0);
ALTER TABLE cost_record ADD COLUMN cache_write_tokens INTEGER NOT NULL DEFAULT 0
    CHECK (cache_write_tokens >= 0);
ALTER TABLE cost_record ADD COLUMN cache_read_tokens INTEGER NOT NULL DEFAULT 0
    CHECK (cache_read_tokens >= 0);

CREATE TABLE rate_new (
    profile_id   TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    provider     TEXT NOT NULL,
    model        TEXT NOT NULL,
    meter        TEXT NOT NULL
                 CHECK (meter IN ('input_tokens', 'output_tokens', 'image_tokens', 'characters',
                                  'video_seconds', 'audio_seconds', 'posts', 'searches',
                                  'cache_write_tokens', 'cache_read_tokens')),
    price_micros INTEGER NOT NULL CHECK (price_micros >= 0),
    PRIMARY KEY (profile_id, provider, model, meter)
) STRICT, WITHOUT ROWID;

INSERT INTO rate_new (profile_id, provider, model, meter, price_micros)
SELECT profile_id, provider, model, meter, price_micros FROM rate;

DROP TABLE rate;

ALTER TABLE rate_new RENAME TO rate;

-- A product's sheet: what Bardo knows about it beyond the brief, written
-- by hand, from the brief, or from a research. One per product, and it
-- goes with the product. Codes and limits are checked by the domain.
CREATE TABLE product_sheet (
    product_id    TEXT PRIMARY KEY NOT NULL REFERENCES product (id) ON DELETE CASCADE,
    summary       TEXT NOT NULL,
    category      TEXT NOT NULL,
    -- A category ad platforms restrict ('health', 'finance', ...), when
    -- the product falls in one.
    restricted    TEXT,
    -- Whether the user changed that category themselves: what they chose
    -- stays, none included, whatever a later run finds.
    restricted_chosen INTEGER NOT NULL DEFAULT 0 CHECK (restricted_chosen IN (0, 1)),
    -- How the product looks, for a product nobody photographed.
    looks         TEXT NOT NULL,
    -- When a research last added to the sheet; Unix time in milliseconds.
    researched_at INTEGER,
    updated_at    INTEGER NOT NULL
) STRICT;

CREATE TABLE product_sheet_segment (
    product_id TEXT NOT NULL REFERENCES product_sheet (product_id) ON DELETE CASCADE,
    position   INTEGER NOT NULL CHECK (position >= 0),
    name       TEXT NOT NULL,
    wants      TEXT NOT NULL,
    holds_back TEXT NOT NULL,
    PRIMARY KEY (product_id, position)
) STRICT, WITHOUT ROWID;

CREATE TABLE product_sheet_objection (
    product_id TEXT NOT NULL REFERENCES product_sheet (product_id) ON DELETE CASCADE,
    position   INTEGER NOT NULL CHECK (position >= 0),
    objection  TEXT NOT NULL,
    answer     TEXT NOT NULL,
    PRIMARY KEY (product_id, position)
) STRICT, WITHOUT ROWID;

CREATE TABLE product_sheet_alternative (
    product_id TEXT NOT NULL REFERENCES product_sheet (product_id) ON DELETE CASCADE,
    position   INTEGER NOT NULL CHECK (position >= 0),
    name       TEXT NOT NULL,
    differs    TEXT NOT NULL,
    PRIMARY KEY (product_id, position)
) STRICT, WITHOUT ROWID;

-- The pages a research read, kept so that a claim found on the web can
-- always be traced to where it was read.
CREATE TABLE product_sheet_source (
    product_id TEXT NOT NULL REFERENCES product_sheet (product_id) ON DELETE CASCADE,
    position   INTEGER NOT NULL CHECK (position >= 0),
    url        TEXT NOT NULL,
    title      TEXT NOT NULL,
    -- Unix time in milliseconds.
    read_at    INTEGER NOT NULL,
    PRIMARY KEY (product_id, position)
) STRICT, WITHOUT ROWID;

-- What an ad may say about the product. A claim keeps its id for as long
-- as it is on the sheet: scripts point at the claims they rest on.
CREATE TABLE product_claim (
    id               TEXT PRIMARY KEY NOT NULL,
    product_id       TEXT NOT NULL REFERENCES product_sheet (product_id) ON DELETE CASCADE,
    position         INTEGER NOT NULL CHECK (position >= 0),
    text             TEXT NOT NULL,
    -- 'benefit', 'feature', 'proof', 'offer' or 'comparison'.
    kind             TEXT NOT NULL,
    -- Where it came from: 'typed', 'brief', 'page' (the product's own) or 'web', the
    -- last two with the page's address (and title, for 'web').
    source           TEXT NOT NULL,
    source_url       TEXT NOT NULL,
    source_title     TEXT NOT NULL,
    -- 'confirmed', 'found' or 'rejected'. Only a confirmed claim reaches
    -- an ad.
    state            TEXT NOT NULL,
    -- Whether the model marked it as one to look at twice.
    flagged          INTEGER NOT NULL CHECK (flagged IN (0, 1)),
    -- The user's own say on that, when they gave one.
    sensitive_choice INTEGER CHECK (sensitive_choice IN (0, 1)),
    -- Unix times in milliseconds.
    confirmed_at     INTEGER,
    edited_at        INTEGER,
    created_at       INTEGER NOT NULL
) STRICT;

CREATE INDEX product_claim_product ON product_claim (product_id, position);
