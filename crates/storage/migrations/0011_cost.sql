-- What each paid provider call cost: what the provider counted, per meter,
-- and the amount it reported or the rate table gave when it happened, in
-- millionths of a US dollar. Never changes once saved, and stays when its
-- channel or video project goes: the money was spent.
CREATE TABLE cost_record (
    id            TEXT PRIMARY KEY NOT NULL,
    profile_id    TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    provider      TEXT NOT NULL,
    model         TEXT NOT NULL,
    purpose       TEXT NOT NULL,
    input_tokens  INTEGER NOT NULL CHECK (input_tokens >= 0),
    output_tokens INTEGER NOT NULL CHECK (output_tokens >= 0),
    image_tokens  INTEGER NOT NULL CHECK (image_tokens >= 0),
    characters    INTEGER NOT NULL CHECK (characters >= 0),
    basis         TEXT NOT NULL CHECK (basis IN ('reported', 'estimated', 'unpriced')),
    amount_micros INTEGER NOT NULL CHECK (amount_micros >= 0),
    -- No foreign keys: records outlive what they were for.
    channel_id    TEXT,
    project_id    TEXT,
    job_id        TEXT,
    -- Unix time in milliseconds.
    at            INTEGER NOT NULL,
    CHECK (basis <> 'unpriced' OR amount_micros = 0)
) STRICT;

CREATE INDEX cost_record_time ON cost_record (profile_id, at);
CREATE INDEX cost_record_project ON cost_record (project_id);
CREATE INDEX cost_record_purpose ON cost_record (profile_id, purpose, at);

-- The user's changes to the built-in rate table: the price of one meter for
-- a provider's models whose name starts with `model`, in millionths of a
-- US dollar per million tokens or per thousand characters.
CREATE TABLE rate (
    profile_id   TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    provider     TEXT NOT NULL,
    model        TEXT NOT NULL,
    meter        TEXT NOT NULL
                 CHECK (meter IN ('input_tokens', 'output_tokens', 'image_tokens', 'characters')),
    price_micros INTEGER NOT NULL CHECK (price_micros >= 0),
    PRIMARY KEY (profile_id, provider, model, meter)
) STRICT, WITHOUT ROWID;

-- A provider's monthly budget, in millionths of a US dollar.
CREATE TABLE budget (
    profile_id     TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    provider       TEXT NOT NULL,
    monthly_micros INTEGER NOT NULL CHECK (monthly_micros >= 0),
    PRIMARY KEY (profile_id, provider)
) STRICT, WITHOUT ROWID;
