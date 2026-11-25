-- Products (#273): the library of things to advertise, owned by the user
-- profile like personas. A product is its brief, up to three images and
-- an optional logo. The pictures are kept in the database beside the
-- product, like a persona's look, so every backup and restore carries
-- them together. Codes and limits are checked by the domain.
CREATE TABLE product (
    id             TEXT PRIMARY KEY NOT NULL,
    profile_id     TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    name           TEXT NOT NULL,
    -- 'digital', 'physical', 'service' or 'idea'.
    kind           TEXT NOT NULL,
    -- What it is.
    description    TEXT NOT NULL,
    -- Who it is for.
    audience       TEXT NOT NULL,
    -- The facts the user stands behind, one per line.
    facts          TEXT NOT NULL,
    offer          TEXT NOT NULL,
    link           TEXT NOT NULL,
    call_to_action TEXT NOT NULL,
    -- 'mine' or 'client'.
    ownership      TEXT NOT NULL,
    must_say       TEXT NOT NULL,
    never_say      TEXT NOT NULL,
    archived       INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
    created_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

-- Backstop for the app's own check; NOCASE folds ASCII only.
CREATE UNIQUE INDEX product_profile_name ON product (profile_id, name COLLATE NOCASE);

-- The user's own files, byte for byte.
CREATE TABLE product_image (
    id           TEXT PRIMARY KEY NOT NULL,
    product_id   TEXT NOT NULL REFERENCES product (id) ON DELETE CASCADE,
    -- Where it sits among the product's images: three at most.
    position     INTEGER NOT NULL CHECK (position BETWEEN 0 AND 2),
    -- 'photo' or 'screen', as the user marks it.
    kind         TEXT NOT NULL,
    image        BLOB NOT NULL,
    format       TEXT NOT NULL,
    width        INTEGER NOT NULL CHECK (width > 0),
    height       INTEGER NOT NULL CHECK (height > 0),
    -- When the user confirmed they may use the image and the brand; Unix
    -- time in milliseconds.
    confirmed_at INTEGER NOT NULL,
    UNIQUE (product_id, position)
) STRICT;

CREATE TABLE product_logo (
    product_id   TEXT PRIMARY KEY NOT NULL REFERENCES product (id) ON DELETE CASCADE,
    image        BLOB NOT NULL,
    width        INTEGER NOT NULL CHECK (width > 0),
    height       INTEGER NOT NULL CHECK (height > 0),
    -- Unix time in milliseconds.
    confirmed_at INTEGER NOT NULL
) STRICT;
