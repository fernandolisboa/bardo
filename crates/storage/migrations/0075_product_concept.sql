-- Concept products (#280, ADR-0043): a concept image drawn for a product
-- of kind 'idea' with no image of its own. One may wait for the user's
-- review ('pending') beside the kept one ('kept'); keeping replaces the
-- kept one. The picture is the drawing, byte for byte.
CREATE TABLE product_concept (
    id              TEXT PRIMARY KEY NOT NULL,
    product_id      TEXT NOT NULL REFERENCES product (id) ON DELETE CASCADE,
    slot            TEXT NOT NULL CHECK (slot IN ('kept', 'pending')),
    image           BLOB NOT NULL,
    format          TEXT NOT NULL,
    width           INTEGER NOT NULL CHECK (width > 0),
    height          INTEGER NOT NULL CHECK (height > 0),
    -- Claude's prompt the image was drawn from.
    prompt          TEXT NOT NULL,
    template_id     TEXT,
    template_number INTEGER,
    writer_model    TEXT NOT NULL,
    image_model     TEXT NOT NULL,
    -- The job that drew it.
    job_id          TEXT,
    -- Unix time in milliseconds.
    drawn_at        INTEGER NOT NULL,
    UNIQUE (product_id, slot)
) STRICT;

-- Whether an ad project is a concept: its product was of kind 'idea' when
-- the project started. Projects started before this read their product's
-- kind as it is now.
ALTER TABLE ad_brief ADD COLUMN concept INTEGER NOT NULL DEFAULT 0;
UPDATE ad_brief SET concept = 1
    WHERE product_id IN (SELECT id FROM product WHERE kind = 'idea');
