-- World: places and objects, and characters' belongings (issue #338,
-- ADR-0047 item 6).

-- The `character` table holds every world item: a character, a place or an
-- object. Names are unique per owner and kind, ignoring case.
ALTER TABLE character ADD COLUMN kind TEXT NOT NULL DEFAULT 'character'
    CHECK (kind IN ('character', 'place', 'object'));

-- The character an object belongs to; deleting the character leaves the
-- object, belonging to nobody.
ALTER TABLE character ADD COLUMN belongs_to TEXT
    REFERENCES character (id) ON DELETE SET NULL;

DROP INDEX character_name;
CREATE UNIQUE INDEX character_name
    ON character (profile_id, IFNULL(owner_project_id, ''), kind, name COLLATE NOCASE);

-- When the user confirmed they may use an imported picture (Unix time in
-- milliseconds); NULL for a drawn one. An imported picture has no provider
-- ('none'), model or prompt.
ALTER TABLE reference_picture ADD COLUMN confirmed_at INTEGER;

-- A plan's place that is a world place; NULL for a place of the video.
-- No foreign key, as for scene_cast.character_id.
ALTER TABLE scene_place ADD COLUMN item_id TEXT;

-- The world objects a scene shows: their ids, comma separated.
ALTER TABLE scene ADD COLUMN objects TEXT NOT NULL DEFAULT '';
