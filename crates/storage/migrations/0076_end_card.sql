-- An ad's end card (#281, ADR-0044): the card's own design, one per
-- project, saved once the user changes it (until then it starts from the
-- Brief stage). The card's backdrop, image and how it is set are the end
-- card scene's shot look.
CREATE TABLE end_card (
    project_id  TEXT PRIMARY KEY NOT NULL REFERENCES video_project (id) ON DELETE CASCADE,
    headline    TEXT NOT NULL,
    second_line TEXT NOT NULL,
    -- Whether the card shows the Brief stage's offer.
    offer       INTEGER NOT NULL CHECK (offer IN (0, 1)),
    look        TEXT NOT NULL,
    text_place  TEXT NOT NULL,
    logo_place  TEXT NOT NULL,
    -- Unix time in milliseconds.
    updated_at  INTEGER NOT NULL
) STRICT;

-- The end card design a scene shows, and the one its composed picture was
-- made with, by fingerprint: a picture of another design is out of date.
-- An end card's picture may show no product image (a card of text).
ALTER TABLE scene ADD COLUMN card TEXT;
ALTER TABLE scene ADD COLUMN shot_card TEXT;
