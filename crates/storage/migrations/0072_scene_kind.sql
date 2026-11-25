-- Scene kinds of an ad's plan and composed product shots (#278,
-- ADR-0040).
--
-- What the scene is: 'drawn', 'product_in_use', 'product_shot' or
-- 'end_card'. Every scene made before, and every scene of a plan that is
-- no ad's, is a drawn scene. Codes are checked by the domain.
ALTER TABLE scene ADD COLUMN kind TEXT NOT NULL DEFAULT 'drawn';
-- The product image a product shot or an end card shows. No reference:
-- a picture composed from an image the product no longer has stays the
-- scene's picture.
ALTER TABLE scene ADD COLUMN product_image_id TEXT;
-- The picture composed for the scene: its file in the project folder,
-- the product image it shows and the frame's shape it was made for
-- ('9:16' or '16:9'). All three are set, or none.
ALTER TABLE scene ADD COLUMN shot_file TEXT;
ALTER TABLE scene ADD COLUMN shot_image_id TEXT;
ALTER TABLE scene ADD COLUMN shot_aspect TEXT;
