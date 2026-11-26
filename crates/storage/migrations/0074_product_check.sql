-- The product in use (#279, ADR-0042): which of a scene's images were
-- drawn with the product's photo as a reference, and the image and clip
-- the user checked against the real product. One line, read by the
-- storage adapter: "<image drawn>,<pending drawn>,<checked image
-- generation>,<checked clip generation>", with 0 or 1 for the first two
-- and '-' for no check. NULL is a scene that holds none of it.
ALTER TABLE scene ADD COLUMN product_checks TEXT;
